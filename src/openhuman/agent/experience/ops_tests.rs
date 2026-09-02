use super::*;

#[test]
fn profile_memory_subdir_matches_live_session_derivation() {
    let workspace = tempfile::TempDir::new().unwrap();
    let mut profile = crate::openhuman::agent::profiles::store::built_in_default_profile();
    profile.id = "alice".into();
    profile.name = "Alice".into();
    profile.built_in = false;
    profile.is_master = false;
    profile.dedicated_memory = true;
    crate::openhuman::agent::profiles::store::AgentProfileStore::new(
        workspace.path().to_path_buf(),
    )
    .upsert(profile)
    .expect("seed profile");

    assert_eq!(
        profile_memory_subdir(workspace.path(), Some("alice")).unwrap(),
        "memory-alice"
    );
    assert_eq!(
        profile_memory_subdir(workspace.path(), None).unwrap(),
        "memory"
    );
    assert!(profile_memory_subdir(workspace.path(), Some("missing")).is_err());

    assert_eq!(
        query_memory_subdirs(workspace.path(), Some("alice")).unwrap(),
        vec!["memory".to_string(), "memory-alice".to_string()]
    );
    assert_eq!(
        query_memory_subdirs(workspace.path(), None).unwrap(),
        vec!["memory".to_string(), "memory-alice".to_string()]
    );
}

/// A config on a disposable workspace with the in-process TinyCortex driver
/// bound to it.
///
/// A test workspace with nothing installed resolves to the **null** driver,
/// which serves nothing and discards writes — so a round-trip through
/// [`DriverMemory`] would fail in a way that looks like a bug in the
/// adapter. TinyCortex is the engine the loadable module wraps, so binding
/// it exercises the same store production reaches over the bus, and unlike
/// the module it is not a process singleton.
fn bound_config() -> (tempfile::TempDir, Config) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();
    config.config_path = tmp.path().join("config.toml");
    // Keep the round-trip hermetic: `"none"` routes to the inert embedder
    // rather than a live embedding service.
    config.embeddings_provider = Some("none".into());
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&config);
    (tmp, config)
}

/// The adapter is a real `Memory` over the bound driver, not a shape that
/// merely compiles: what `store` writes, `get` and `list` read back.
#[tokio::test]
async fn driver_memory_round_trips_through_the_bound_driver() {
    let (_tmp, config) = bound_config();
    let memory = DriverMemory::for_subtree(&config, "memory").expect("bind driver");

    // `name()` is the *driver's* id, so this doubles as the fixture's own
    // canary: a workspace with no binding installed resolves to the null
    // driver, which accepts writes and returns nothing — the round-trip
    // below would then fail for the wrong reason.
    assert_ne!(
        memory.name(),
        tinymemory_api::null::NULL_DRIVER_ID,
        "the adapter must be over the bound driver, not the null fallback"
    );

    memory
        .store(
            "ops_adapter_ns",
            "k1",
            "adapter round trip",
            MemoryCategory::Custom("ops_adapter_ns".into()),
            None,
        )
        .await
        .expect("store");

    let fetched = memory.get("ops_adapter_ns", "k1").await.expect("get");
    assert_eq!(
        fetched.map(|entry| entry.content).as_deref(),
        Some("adapter round trip")
    );

    let listed = memory
        .list(Some("ops_adapter_ns"), None, None)
        .await
        .expect("list");
    assert!(listed.iter().any(|entry| entry.key == "k1"));

    assert!(memory.health_check().await, "a bound driver is reachable");
}

/// The experience store opened for a profile-less caller and the one opened
/// for the `"memory"` subtree are the same store — the shared-tree arm has
/// no special case any more.
#[tokio::test]
async fn experience_store_round_trips_over_the_bound_driver() {
    let (_tmp, config) = bound_config();
    let store = open_store_in_subdir(&config, "memory")
        .await
        .expect("open store");

    let experience = AgentExperience {
        id: "exp_adapter".into(),
        created_at_ms: 1,
        updated_at_ms: 1,
        source: crate::openhuman::agent::experience::types::ExperienceSource::ToolLoop,
        agent_id: None,
        entrypoint: None,
        profile_id: None,
        task_fingerprint: "fp-adapter".into(),
        task_summary: "route the experience store onto the contract".into(),
        tools_used: Vec::new(),
        tool_sequence: Vec::new(),
        outcome: crate::openhuman::agent::experience::types::ExperienceOutcome::Success,
        error_class: None,
        lesson: "the bound driver is the store".into(),
        reuse_hint: "open it through the binding".into(),
        avoid_hint: None,
        confidence: 0.9,
        tags: Vec::new(),
        payload_hash: None,
        dismissed: false,
    };

    store.put(experience).await.expect("put");
    let listed = store.list().await.expect("list");
    assert!(
        listed.iter().any(|item| item.id == "exp_adapter"),
        "a record written through the bound driver must read back"
    );
}
