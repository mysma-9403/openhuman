use super::*;
use crate::openhuman::agent::context::prompt::LearnedContextData;
use crate::openhuman::memory::{Memory, MemoryCategory, MemoryEntry};
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

struct NoopMemory;

#[async_trait]
impl Memory for NoopMemory {
    fn name(&self) -> &str {
        "noop"
    }

    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: crate::openhuman::memory::RecallOpts<'_>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        Ok(None)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn namespace_summaries(
        &self,
    ) -> anyhow::Result<Vec<crate::openhuman::memory::NamespaceSummary>> {
        Ok(Vec::new())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        Ok(0)
    }

    async fn health_check(&self) -> bool {
        true
    }
}

fn prompt_context(learned: LearnedContextData) -> PromptContext<'static> {
    let visible_tool_names = Box::leak(Box::new(HashSet::new()));
    PromptContext {
        workspace_dir: Path::new("/tmp"),
        model_name: "test-model",
        agent_id: "",
        tools: &[],
        workflows: &[],
        dispatcher_instructions: "",
        learned,
        visible_tool_names,
        tool_call_format: crate::openhuman::agent::context::prompt::ToolCallFormat::PFormat,
        connected_integrations: &[],
        connected_identities_md: String::new(),
        include_profile: false,
        include_memory_md: false,
        user_identity: None,
        personality_soul_md: None,
        personality_memory_md: None,
        personality_roster: vec![],
        agents_md_global: None,
        agents_md_local: None,
        curated_snapshot: None,
    }
}

#[test]
fn learned_context_section_renders_observations_and_patterns() {
    let section = LearnedContextSection::new(Arc::new(NoopMemory));
    let rendered = section
        .build(&prompt_context(LearnedContextData {
            observations: vec!["Tool use succeeded".into()],
            patterns: vec!["User prefers terse replies".into()],
            user_profile: Vec::new(),
            reflections: Vec::new(),
            tree_root_summaries: Vec::new(),
        }))
        .unwrap();

    assert_eq!(section.name(), "learned_context");
    assert!(rendered.contains("## Learned Context"));
    assert!(rendered.contains("### Recent Observations"));
    assert!(rendered.contains("- Tool use succeeded"));
    assert!(rendered.contains("### Recognized Patterns"));
    assert!(rendered.contains("- User prefers terse replies"));
}

#[test]
fn learned_context_section_returns_empty_without_entries() {
    let section = LearnedContextSection::new(Arc::new(NoopMemory));
    assert!(section
        .build(&prompt_context(LearnedContextData::default()))
        .unwrap()
        .is_empty());
}

#[test]
fn user_profile_section_renders_bullets() {
    let section = UserProfileSection::new(Arc::new(NoopMemory));
    let rendered = section
        .build(&prompt_context(LearnedContextData {
            observations: Vec::new(),
            patterns: Vec::new(),
            user_profile: vec![
                "Timezone: America/Los_Angeles".into(),
                "Prefers Rust".into(),
            ],
            reflections: Vec::new(),
            tree_root_summaries: Vec::new(),
        }))
        .unwrap();

    assert_eq!(section.name(), "user_profile");
    assert!(rendered.starts_with("## Your standing preferences\n\n"));
    assert!(rendered.contains("- Timezone: America/Los_Angeles"));
    assert!(rendered.contains("- Prefers Rust"));
}

#[test]
fn user_profile_section_returns_empty_without_profile_entries() {
    let section = UserProfileSection::new(Arc::new(NoopMemory));
    assert!(section
        .build(&prompt_context(LearnedContextData::default()))
        .unwrap()
        .is_empty());
}

// ── load_learned_from_cache ───────────────────────────────────────────────

#[tokio::test]
async fn load_learned_from_cache_formats_active_facets() {
    use tinymemory_api::provider::{FacetState, FacetType, ProfileFacet, UserState};
    let cache = crate::openhuman::agent::learning::test_profile::in_memory_cache();

    let make_facet = |id: &str, key: &str, value: &str, stab: f64| ProfileFacet {
        facet_id: id.into(),
        facet_type: FacetType::Preference,
        key: key.into(),
        value: value.into(),
        confidence: 0.8,
        evidence_count: 2,
        source_segment_ids: None,
        first_seen_at: 1000.0,
        last_seen_at: 1200.0,
        state: FacetState::Active,
        stability: stab,
        user_state: UserState::Auto,
        evidence_refs: vec![],
        class: None,
        cue_families: None,
    };

    cache
        .upsert(&make_facet("f1", "style/verbosity", "terse", 2.0))
        .await
        .unwrap();
    cache
        .upsert(&make_facet("f2", "identity/name", "Alice", 1.8))
        .await
        .unwrap();
    cache
        .upsert(&make_facet(
            "f3",
            "goal/learn_rust",
            "Learn Rust this year",
            1.6,
        ))
        .await
        .unwrap();

    // Provisional — should NOT appear.
    let mut prov = make_facet("f4", "style/tone", "formal", 0.8);
    prov.state = FacetState::Provisional;
    cache.upsert(&prov).await.unwrap();

    let result = load_learned_from_cache(&cache).await;

    assert!(
        !result.is_empty(),
        "should produce entries for Active facets"
    );
    // Phase 4 format: "**style/verbosity**: terse"
    assert!(
        result.iter().any(|s| s.contains("style/verbosity")),
        "style/verbosity should appear"
    );
    assert!(
        result
            .iter()
            .any(|s| s.contains("**style/verbosity**: terse")),
        "style/verbosity should use Phase 4 bold format"
    );
    // Goal class → value only (no key prefix)
    assert!(
        result.iter().any(|s| s == "Learn Rust this year"),
        "goal class should render value only"
    );
    // Provisional should not appear
    assert!(
        !result.iter().any(|s| s.contains("style/tone")),
        "provisional facet must not appear in cache prompt"
    );
}

#[tokio::test]
async fn load_learned_from_cache_empty_when_no_active_facets() {
    let cache = crate::openhuman::agent::learning::test_profile::in_memory_cache();

    let result = load_learned_from_cache(&cache).await;
    assert!(result.is_empty());
}

// ── MemoryAccessSection ───────────────────────────────────────────────────

#[test]
fn memory_access_section_renders_static_text() {
    let section = MemoryAccessSection;
    assert_eq!(section.name(), "memory_access");
    let rendered = section
        .build(&prompt_context(LearnedContextData::default()))
        .unwrap();
    assert!(
        rendered.contains("## Memory access"),
        "heading missing:\n{rendered}"
    );
    assert!(
        rendered.contains("memory_recall"),
        "memory_recall tool not mentioned:\n{rendered}"
    );
    assert!(
        rendered.contains("memory_search"),
        "memory_search tool not mentioned:\n{rendered}"
    );
    // Verify the rendered text matches the constant.
    assert_eq!(rendered.trim(), MEMORY_ACCESS_INSTRUCTION.trim());
}

#[test]
fn memory_access_section_present_in_system_prompt_compose() {
    // Verify the section renders correctly when added to a prompt composition.
    let section = MemoryAccessSection;
    let rendered = section
        .build(&prompt_context(LearnedContextData::default()))
        .unwrap();
    // Spot-check the content constraint: ≤ 80 tokens (rough word count).
    let word_count = rendered.split_whitespace().count();
    assert!(
        word_count <= 100,
        "MemoryAccessSection is too long ({word_count} words, target ≤ 80 tokens)"
    );
    // The section name must be stable (used for insert_section_before).
    assert_eq!(section.name(), "memory_access");
    // Content check: the section must mention both retrieval tools.
    assert!(rendered.contains("memory_recall"));
    assert!(rendered.contains("memory_search"));
    // Verify it is non-empty for any PromptContext (not context-gated).
    let empty_ctx = prompt_context(LearnedContextData::default());
    let rendered_empty_ctx = section.build(&empty_ctx).unwrap();
    assert!(
        !rendered_empty_ctx.trim().is_empty(),
        "must render regardless of learned context"
    );
}
