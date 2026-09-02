use super::*;
use tempfile::tempdir;

/// Minimal custom profile literal for tests, with all allowlists unset.
fn custom(id: &str, name: &str, agent_id: &str) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        agent_id: agent_id.into(),
        model_override: None,
        temperature: None,
        system_prompt_suffix: None,
        allowed_tools: None,
        built_in: false,
        avatar_url: None,
        voice_id: None,
        soul_md: None,
        soul_md_path: None,
        composio_integrations: None,
        memory_sources: None,
        include_agent_conversations: true,
        allowed_skills: None,
        allowed_mcp_servers: None,
        memory_dir_suffix: None,
        is_master: false,
        sort_order: None,
        dedicated_memory: false,
        dedicated_workspace: false,
    }
}

#[test]
fn profile_store_roundtrips_active_profile_and_custom_entries() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let mut profile = custom(" Custom Profile ", " Custom Profile ", " planner ");
    profile.description = "  My custom profile ".into();
    profile.model_override = Some(" agentic-v1 ".into());
    profile.temperature = Some(0.25);
    profile.system_prompt_suffix = Some("  Be brief. ".into());
    profile.allowed_tools = Some(vec![" todo ".into(), "".into()]);
    let state = store.upsert(profile).expect("upsert");
    assert!(state.profiles.iter().any(|p| p.id == "custom-profile"));

    let selected = store.select("custom-profile").expect("select");
    assert_eq!(selected.active_profile_id, "custom-profile");

    let loaded = store.load().expect("load");
    let custom = loaded
        .profiles
        .iter()
        .find(|profile| profile.id == "custom-profile")
        .expect("custom profile");
    assert_eq!(custom.agent_id, "planner");
    assert_eq!(
        custom.allowed_tools.as_deref(),
        Some(vec!["todo".to_string()].as_slice())
    );

    let resolved = store.resolve(Some("custom-profile")).expect("resolve").1;
    assert_eq!(resolved.id, "custom-profile");
}

#[test]
fn normalise_state_heals_stale_numeric_suffix_on_built_in_profiles() {
    // An earlier `upsert` bug stamped the built-in `reasoning` profile with a
    // scoped `memory_dir_suffix`, isolating its transcripts + memory into
    // `session_raw-1/` / `memory-1/` and dropping context on a mid-thread
    // Quick↔Reasoning switch (#5351). Loading must heal it back to the shared
    // subtree; a genuine custom profile's suffix must be left alone.
    let mut reasoning = custom("reasoning", "Reasoning", "orchestrator");
    reasoning.built_in = true;
    reasoning.memory_dir_suffix = Some("-1".into());
    let mut sidekick = custom("sidekick", "Sidekick", "orchestrator");
    sidekick.memory_dir_suffix = Some("-2".into());

    let state = normalise_state(AgentProfilesState {
        active_profile_id: DEFAULT_PROFILE_ID.into(),
        profiles: vec![reasoning, sidekick],
    });

    let reasoning = state
        .profiles
        .iter()
        .find(|p| p.id == "reasoning")
        .expect("reasoning present");
    assert_eq!(
        reasoning.memory_dir_suffix, None,
        "built-in reasoning must be pinned to the shared memory/session_raw subtree"
    );
    let sidekick = state
        .profiles
        .iter()
        .find(|p| p.id == "sidekick")
        .expect("custom sidekick present");
    assert_eq!(
        sidekick.memory_dir_suffix.as_deref(),
        Some("-2"),
        "a genuine custom profile's isolated-memory suffix must be preserved"
    );
}

#[test]
fn upsert_of_built_in_reasoning_stays_on_shared_subtree() {
    // Editing + saving the built-in Reasoning profile must NOT scope its
    // memory: it ships `dedicated_memory:false` and shares the default
    // subtree, so a mid-thread switch keeps context (#5351).
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let mut reasoning = custom("reasoning", "Reasoning", "orchestrator");
    reasoning.built_in = true;
    reasoning.model_override = Some("hint:reasoning".into());
    // ProfileEditorPage submits without a `memory_dir_suffix`.
    reasoning.memory_dir_suffix = None;

    store.upsert(reasoning).expect("upsert reasoning");

    let loaded = store.load().expect("load");
    let reasoning = loaded
        .profiles
        .iter()
        .find(|p| p.id == "reasoning")
        .expect("reasoning present");
    assert_eq!(
        reasoning.memory_dir_suffix, None,
        "upsert must never stamp a built-in profile with a scoped memory suffix"
    );
}

#[test]
fn upsert_rejects_profile_ids_longer_than_home_path_limit() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let profile = custom(&"a".repeat(65), "Overlong", "orchestrator");

    let error = store
        .upsert(profile)
        .expect_err("overlong id must be rejected");
    assert!(error.contains("too long"));
    assert!(store
        .load()
        .expect("load")
        .profiles
        .iter()
        .all(|profile| profile.name != "Overlong"));
}

#[test]
fn built_in_profiles_are_merged_when_file_is_missing() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let loaded = store.load().expect("load");
    let ids: Vec<&str> = loaded.profiles.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&DEFAULT_PROFILE_ID));
    assert!(ids.contains(&"research"));
    assert_eq!(loaded.active_profile_id, DEFAULT_PROFILE_ID);
}

#[test]
fn load_profiles_helper_reads_defaults() {
    let dir = tempdir().expect("tempdir");
    let loaded = load_profiles(dir.path()).expect("load profiles");
    assert!(loaded
        .profiles
        .iter()
        .any(|profile| profile.id == DEFAULT_PROFILE_ID));
}

#[test]
fn normalise_state_falls_back_to_default_active_profile() {
    let mut bad = custom("  ", "   ", " ");
    bad.description = " ignored ".into();
    bad.model_override = Some(" ".into());
    bad.system_prompt_suffix = Some(" ".into());
    bad.allowed_tools = Some(vec![" ".into()]);
    let state = normalise_state(AgentProfilesState {
        active_profile_id: "missing".into(),
        profiles: vec![bad],
    });

    assert_eq!(state.active_profile_id, DEFAULT_PROFILE_ID);
    assert!(!state.profiles.iter().any(|profile| profile.id.is_empty()));
}

#[test]
fn normalise_profile_drops_empty_allowlists_to_none() {
    let mut profile = custom("scoped", "Scoped", "orchestrator");
    profile.memory_sources = Some(vec![" ".into(), "".into()]);
    profile.allowed_skills = Some(vec![" deep-research ".into()]);
    profile.allowed_mcp_servers = Some(vec![]);
    profile.composio_integrations = Some(vec!["  ".into()]);
    let normalised = normalise_profile(profile);
    assert_eq!(normalised.memory_sources, None);
    assert_eq!(
        normalised.allowed_skills.as_deref(),
        Some(vec!["deep-research".to_string()].as_slice())
    );
    assert_eq!(normalised.allowed_mcp_servers, None);
    assert_eq!(normalised.composio_integrations, None);
}

#[test]
fn upsert_default_profile_preserves_builtin_default_identity() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let mut profile = custom(DEFAULT_PROFILE_ID, " Default Custom ", " planner ");
    profile.description = " custom description ".into();
    profile.model_override = Some(" agentic-v1 ".into());
    profile.temperature = Some(0.3);
    profile.system_prompt_suffix = Some(" suffix ".into());
    profile.allowed_tools = Some(vec![" todo ".into()]);
    profile.memory_sources = Some(vec!["slack-eng".into()]);
    profile.dedicated_memory = true;
    profile.dedicated_workspace = true;
    let state = store.upsert(profile).expect("upsert default");
    let default = state
        .profiles
        .iter()
        .find(|profile| profile.id == DEFAULT_PROFILE_ID)
        .expect("default profile");
    assert!(default.built_in);
    assert_eq!(default.agent_id, "orchestrator");
    assert_eq!(default.name, "Default Custom");
    assert_eq!(default.system_prompt_suffix.as_deref(), Some("suffix"));
    // New allowlist fields round-trip through the default merge branch.
    assert_eq!(
        default.memory_sources.as_deref(),
        Some(vec!["slack-eng".to_string()].as_slice())
    );
    assert!(default.dedicated_memory);
    assert!(default.dedicated_workspace);
}

#[test]
fn select_missing_and_delete_builtin_return_errors() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());

    let select_err = store.select("missing").expect_err("missing select");
    assert!(select_err.contains("not found"));

    let delete_err = store
        .delete(DEFAULT_PROFILE_ID)
        .expect_err("builtin delete rejected");
    assert!(delete_err.contains("cannot be deleted"));
}

#[test]
fn delete_missing_custom_profile_returns_error() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let err = store.delete("not-there").expect_err("missing delete");
    assert!(err.contains("not found"));
}

#[test]
fn resolve_uses_active_profile_and_falls_back_to_default() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    store
        .upsert(custom("writer", "Writer", "planner"))
        .expect("upsert");
    store.select("writer").expect("select");

    let active = store.resolve(None).expect("resolve active").1;
    assert_eq!(active.id, "writer");
    let fallback = store.resolve(Some("missing")).expect("resolve missing").1;
    assert_eq!(fallback.id, DEFAULT_PROFILE_ID);
}

#[test]
fn deleting_active_custom_profile_falls_back_to_default() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    store
        .upsert(custom("tmp", "Tmp", "orchestrator"))
        .expect("upsert");
    store.select("tmp").expect("select");
    let state = store.delete("tmp").expect("delete");
    assert_eq!(state.active_profile_id, DEFAULT_PROFILE_ID);
    assert!(!state.profiles.iter().any(|p| p.id == "tmp"));
}

#[test]
fn memory_dir_suffix_auto_assigned_on_upsert() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    // First custom profile gets "-1"
    let state = store
        .upsert(custom("alice", "Alice", "orchestrator"))
        .expect("upsert alice");
    let alice = state.profiles.iter().find(|p| p.id == "alice").unwrap();
    assert_eq!(alice.memory_dir_suffix.as_deref(), Some("-1"));

    // Second custom profile gets "-2"
    let state = store
        .upsert(custom("bob", "Bob", "orchestrator"))
        .expect("upsert bob");
    let bob = state.profiles.iter().find(|p| p.id == "bob").unwrap();
    assert_eq!(bob.memory_dir_suffix.as_deref(), Some("-2"));

    // Delete alice, create charlie — should reuse "-1"
    store.delete("alice").expect("delete alice");
    let state = store
        .upsert(custom("charlie", "Charlie", "orchestrator"))
        .expect("upsert charlie");
    let charlie = state.profiles.iter().find(|p| p.id == "charlie").unwrap();
    assert_eq!(charlie.memory_dir_suffix.as_deref(), Some("-1"));
}

#[test]
fn upsert_roundtrips_dedicated_home_fields() {
    let dir = tempdir().expect("tempdir");
    let store = AgentProfileStore::new(dir.path().to_path_buf());
    let mut profile = custom("iso", "Iso", "orchestrator");
    profile.dedicated_memory = true;
    profile.dedicated_workspace = true;
    store.upsert(profile).expect("upsert");

    let loaded = store.load().expect("load");
    let iso = loaded
        .profiles
        .iter()
        .find(|p| p.id == "iso")
        .expect("iso profile");
    assert!(iso.dedicated_memory);
    assert!(iso.dedicated_workspace);
}

#[test]
fn default_profile_has_master_and_memory_suffix() {
    let default = built_in_default_profile();
    assert!(default.is_master);
    assert_eq!(default.memory_dir_suffix.as_deref(), Some(""));
    assert!(default.include_agent_conversations);
}
