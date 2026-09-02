use super::*;
use crate::openhuman::agent::hooks::{ToolCallRecord, TurnContext};
use crate::openhuman::memory::api::provider::MemoryProvider;
use std::sync::OnceLock;
use tinymemory_core::chat::ChatPrompt;
// Assertion reads go straight at the engine's tables through the same client
// the provider wraps. Production writes through the provider; the *proof* that
// a row landed may still read the store directly — this is a `_tests.rs` file,
// by-path exempt from the direct-refs ratchet, and a raw read cannot be
// satisfied by anything but the row actually existing.
use tinymemory_core::store::{events as ev, fts5, segments as seg, MemoryClient};
use tinymemory_tinycortex::engine::{EngineRuntimeConfig, TinycortexProvider};

static TREE_INGEST_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Runs `fut` with the memory chat provider pinned to a deterministic stub.
///
/// These tree-ingest tests look hermetic but are not. `ingest_chat` builds its
/// **own** chat provider from `Config` — `memory::tinycortex::ingest::context`
/// → `scoring_config` → `build_chat_provider` — so it ignores the
/// `StubChatProvider` wired into the hook and reaches the managed backend over
/// the network. The ingest treats its own failure as non-fatal (logged and
/// swallowed in `tree_ingest.rs`), so a slow or failed call surfaces only as
/// zero tree chunks, which reads as a wrong assertion rather than a network
/// problem. Under a loaded parallel suite that call's timing varies, which is
/// what made these tests flaky.
///
/// `build_chat_runtime` checks this task-local override before building
/// anything, so scoping the whole test body through it keeps the ingest
/// offline and deterministic.
async fn with_stub_chat_provider<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    // `test_override` is task-local, but tree ingest also builds shared
    // runtime state. Keep these integration-style tests isolated from each
    // other so a concurrent provider construction cannot escape the stub.
    let lock = TREE_INGEST_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;
    // Tree ingest builds a memory store, which reaches the embedding seam.
    // Installing it here rather than relying on some earlier test having done
    // so is what makes these deterministic — the `test_override` below still
    // keeps the *chat* side offline, since `build_chat_runtime` checks it
    // before building anything.
    crate::openhuman::memory::host_impls::install_for_tests();
    tinymemory_core::chat::test_override::with_provider(
        Arc::new(tinymemory_core::chat::StaticChatProvider::new("{}")),
        fut,
    )
    .await
}

/// A real TinyCortex provider over a fresh workspace, plus the engine client
/// for raw assertion reads and the tempdir keeping both alive.
///
/// The archivist writes through `Arc<dyn MemoryProvider>` now, so the fixture
/// is the same shape production binds — the in-process driver here, the
/// loaded module there — rather than a bare connection the hook can no longer
/// accept.
fn setup_provider() -> (TempDir, Arc<MemoryClient>, Arc<dyn MemoryProvider>) {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().join("ws");
    let (client, provider) = provider_over(&workspace);
    (tmp, client, provider)
}

/// The same driver over a caller-owned workspace.
///
/// The tree-ingest tests need the provider and their `Config` to share ONE
/// workspace — the hook ingests through the provider while the assertions
/// count chunks through the config, and two tempdirs would make every count
/// read a store nothing wrote to.
fn provider_over(workspace: &std::path::Path) -> (Arc<MemoryClient>, Arc<dyn MemoryProvider>) {
    crate::openhuman::memory::host_impls::install_for_tests();
    let workspace = workspace.to_path_buf();
    std::fs::create_dir_all(&workspace).unwrap();
    let client = Arc::new(MemoryClient::from_workspace_dir(workspace.clone()).unwrap());
    let config = EngineRuntimeConfig {
        workspace_dir: workspace.clone(),
        config_path: workspace.join("config.toml"),
        memory: Default::default(),
        memory_tree: Default::default(),
        scheduler_gate: Default::default(),
        local_ai: Default::default(),
        embeddings_provider: None,
        memory_provider: None,
        default_model: None,
        default_temperature: 0.2,
        output_language: None,
        memory_sources: serde_json::Value::Null,
        // Added by tinymemory#100, which moved the periodic sync loops into the
        // module. A test fixture wants the same "no cadence configured" default
        // the module answers for an older host that sends nothing.
        memory_sync_interval_secs: None,
        composio_mode: String::new(),
        composio_entity_id: String::new(),
        // Added by tinymemory#103: proxied Composio addresses the backend with
        // this. Empty means the host named none, and the request then fails in the
        // HTTP client rather than falling back to a guessed host.
        backend_api_url: String::new(),
    };
    let provider: Arc<dyn MemoryProvider> = Arc::new(TinycortexProvider::new(
        "tinycortex".into(),
        config,
        Arc::clone(&client),
    ));
    (client, provider)
}

// ── Phase 1: LLM recap + finalize-time embedding ─────────────────────────────

/// Stub ChatProvider that returns a fixed recap string without hitting
/// any real LLM, so the test is hermetic.
struct StubChatProvider;

#[async_trait::async_trait]
impl tinymemory_core::chat::ChatProvider for StubChatProvider {
    fn name(&self) -> &str {
        "stub:test"
    }

    async fn chat_for_json(&self, _prompt: &ChatPrompt) -> anyhow::Result<String> {
        Ok("stub recap: discussed Rust ownership model".to_string())
    }

    async fn chat_for_text(&self, _prompt: &ChatPrompt) -> anyhow::Result<String> {
        Ok("stub recap: discussed Rust ownership model".to_string())
    }
}

/// Build an ArchivistHook with a stub ChatProvider injected directly.
/// Uses the test-only `new_with_stubs` constructor to bypass `with_config`.
fn hook_with_stubs(provider: Arc<dyn MemoryProvider>) -> ArchivistHook {
    ArchivistHook::new_with_stubs(provider, Arc::new(StubChatProvider))
}

// ── Phase 2: segment-granularity tree ingest ─────────────────────────────────
//
// The following tests verify:
//   a) No per-turn tree write fires from on_turn_complete (no double-write).
//   b) Exactly ONE tree ingest fires when a segment closes (not N per turn).
//   c) The ingested batch contains all the segment's raw prose turns.
//   d) The `source_id` is the constant "conversations:agent".
//   e) Each leaf message carries session/segment/episodic-span provenance.
//   f) The ingested content is raw prose, NOT the LLM recap.
//   g) flush_open_segment also triggers tree ingest.

use crate::openhuman::config::Config;
use tempfile::TempDir;
use tinymemory_core::store::chunks::store::{count_chunks, list_chunks, ListChunksQuery};

/// Build a Config that points at a temp workspace, suitable for tree-ingest tests.
/// The memory_tree DB and content dir are created under `tmp.path()`.
fn test_config_with_tree() -> (TempDir, Config) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::default();
    cfg.workspace_dir = tmp.path().to_path_buf();
    // Route the embedder to `InertEmbedder`. This is the knob that actually
    // takes ingest offline: the tree reads `memory.embedding_model`
    // (`memory::tinycortex::config::memory_config_from`), which defaults to the
    // CLOUD model `embedding-v1` — so the three `memory_tree.embedding_*` lines
    // below never disabled anything, and ingest was really calling out to the
    // managed embedding service. `ingest_chat`'s failure is swallowed as
    // non-fatal in `tree_ingest.rs`, so a slow or failed call surfaced only as
    // "got 0 chunks", which reads as a broken assertion rather than a network
    // timeout — that is what made these tests flaky under a loaded suite.
    // See `memory::tree_e2e_tests::pipeline_works_with_embeddings_disabled`,
    // which pins that "none" routes to `InertEmbedder`.
    cfg.embeddings_provider = Some("none".into());
    // Kept: these govern the memory_tree-specific embedding path.
    cfg.memory_tree.embedding_endpoint = None;
    cfg.memory_tree.embedding_model = None;
    cfg.memory_tree.embedding_strict = false;
    // Ensure the tree ingest gate is on.
    cfg.learning.chat_to_tree_enabled = true;
    (tmp, cfg)
}

/// Build a hook that has both stub providers AND a real-enough Config wired in,
/// so the Phase 2 tree ingest path is exercised hermetically.
fn hook_with_stubs_and_tree_config(
    provider: Arc<dyn MemoryProvider>,
    cfg: Config,
) -> ArchivistHook {
    ArchivistHook::new_with_stubs_and_config(provider, Arc::new(StubChatProvider), cfg)
}

async fn phase2_no_per_turn_tree_write_inner() {
    let (_tmp, cfg) = test_config_with_tree();
    let (client, provider) = provider_over(&cfg.workspace_dir);
    let conn = client.profile_conn();
    let hook = hook_with_stubs_and_tree_config(provider.clone(), cfg.clone());

    let session = "phase2-no-per-turn";

    // Single turn — no segment close fires, so no tree ingest should happen.
    hook.on_turn_complete(&TurnContext {
        user_message: "What is Rust?".into(),
        assistant_response: "Rust is a systems programming language.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    // Segment is still open (no boundary fired) — tree must have 0 chunks.
    let open_seg = seg::open_segment_for_session(&conn, session).unwrap();
    assert!(
        open_seg.is_some(),
        "Expected an open segment (no boundary should have fired)"
    );

    let chunk_count = count_chunks(&cfg).unwrap();
    assert_eq!(
        chunk_count, 0,
        "Expected 0 tree chunks after a single turn (no segment close): \
         per-turn tree write must not exist (Phase 2)"
    );
}

async fn phase2_exactly_one_tree_ingest_per_segment_close_inner() {
    let (_tmp, cfg) = test_config_with_tree();
    let (client, provider) = provider_over(&cfg.workspace_dir);
    let hook = hook_with_stubs_and_tree_config(provider.clone(), cfg.clone());

    let session = "phase2-one-ingest";

    // Turn 1 — opens first segment.
    hook.on_turn_complete(&TurnContext {
        user_message: "Tell me about Rust ownership".into(),
        assistant_response: "Rust ownership prevents memory bugs.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    // Turn 2 — stays in same segment.
    hook.on_turn_complete(&TurnContext {
        user_message: "What about the borrow checker?".into(),
        assistant_response: "The borrow checker enforces ownership at compile time.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    })
    .await
    .unwrap();

    // No tree write yet — segment still open.
    let pre_close_chunks = count_chunks(&cfg).unwrap();
    assert_eq!(
        pre_close_chunks, 0,
        "Expected 0 tree chunks before any segment close; got {pre_close_chunks}"
    );

    // Turn 3 — topic change triggers boundary → closes first segment → tree ingest fires.
    hook.on_turn_complete(&TurnContext {
        user_message: "Switching to a completely different topic: tell me about Python asyncio."
            .into(),
        assistant_response: "Python asyncio enables concurrent coroutines.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 3,
    })
    .await
    .unwrap();

    // Segment closed → exactly one ingest for the closed segment (containing turns 1+2).
    // The ingest packs the messages into one or more chunks (greedy packing),
    // but chunks_written >= 1 confirms ingest happened.
    let post_close_chunks = count_chunks(&cfg).unwrap();
    assert!(
        post_close_chunks >= 1,
        "Expected ≥ 1 tree chunk after segment close; got {post_close_chunks}"
    );

    // List the chunks and check they come from the constant source_id.
    let chunks = list_chunks(
        &cfg,
        &ListChunksQuery {
            source_id: Some("conversations:agent".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        !chunks.is_empty(),
        "Expected chunks under source_id='conversations:agent'"
    );
}

async fn phase2_provenance_stamped_on_leaf_and_source_id_is_constant_inner() {
    let (_tmp, cfg) = test_config_with_tree();
    let (client, provider) = provider_over(&cfg.workspace_dir);
    let conn = client.profile_conn();
    let hook = hook_with_stubs_and_tree_config(provider.clone(), cfg.clone());

    let session = "phase2-provenance";

    // Two turns in the first segment.
    for i in 1..=2 {
        hook.on_turn_complete(&TurnContext {
            user_message: format!("Ownership question {i}"),
            assistant_response: format!("Ownership answer {i}"),
            tool_calls: vec![],
            turn_duration_ms: 50,
            session_id: Some(session.into()),
            agent_id: None,
            entrypoint: None,
            iteration_count: i,
        })
        .await
        .unwrap();
    }

    // Force a segment close via flush_open_segment.
    hook.flush_open_segment(session).await;

    // Retrieve the closed segment to extract its ID.
    let all_segs = seg::segments_by_namespace(&conn, "global", 10).unwrap();
    let closed = all_segs
        .iter()
        .find(|s| {
            s.session_id == session
                && s.status != tinymemory_core::store::segments::SegmentStatus::Open
        })
        .expect("Expected a closed segment after flush");

    let segment_id = &closed.segment_id;
    let start_ep = closed.start_episodic_id;
    let end_ep = closed.end_episodic_id.unwrap_or(start_ep);

    // Chunks should be present.
    let chunks = list_chunks(&cfg, &ListChunksQuery::default()).unwrap();
    assert!(
        !chunks.is_empty(),
        "Expected tree chunks after flush_open_segment"
    );

    // source_id must be the constant — never per-session or per-segment.
    for chunk in &chunks {
        assert_eq!(
            chunk.metadata.source_id, "conversations:agent",
            "source_id must be the constant 'conversations:agent', got: {}",
            chunk.metadata.source_id
        );
    }

    // The source_ref on at least one chunk must contain the provenance pattern.
    let expected_provenance =
        format!("agent://session/{session}/segment/{segment_id}#ep{start_ep}-{end_ep}");
    let has_provenance = chunks.iter().any(|chunk| {
        chunk
            .metadata
            .source_ref
            .as_ref()
            .map(|r| {
                r.value
                    .contains(&format!("agent://session/{session}/segment/{segment_id}"))
            })
            .unwrap_or(false)
    });
    assert!(
        has_provenance,
        "Expected at least one chunk with source_ref containing provenance pattern \
         '{expected_provenance}'; found: {:?}",
        chunks
            .iter()
            .map(|c| c.metadata.source_ref.as_ref().map(|r| r.value.as_str()))
            .collect::<Vec<_>>()
    );
}

async fn phase2_ingested_content_is_raw_prose_not_recap_inner() {
    let (_tmp, cfg) = test_config_with_tree();
    let (client, provider) = provider_over(&cfg.workspace_dir);
    let hook = hook_with_stubs_and_tree_config(provider.clone(), cfg.clone());

    let session = "phase2-raw-prose";

    // The stub recap always returns "stub recap: discussed Rust ownership model".
    // The raw user messages contain very different text.
    let user_msg = "My specific question about lifetimes in Rust code";
    let asst_msg = "Lifetimes annotate how long references are valid in memory";

    hook.on_turn_complete(&TurnContext {
        user_message: user_msg.into(),
        assistant_response: asst_msg.into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    // Flush to close the segment and trigger tree ingest.
    hook.flush_open_segment(session).await;

    let chunks = list_chunks(&cfg, &ListChunksQuery::default()).unwrap();
    assert!(
        !chunks.is_empty(),
        "Expected tree chunks after flush_open_segment"
    );

    // The stub recap text must NOT appear in any chunk body.
    let stub_recap_text = "stub recap: discussed Rust ownership model";
    for chunk in &chunks {
        assert!(
            !chunk.content.contains(stub_recap_text),
            "Chunk content must NOT contain the recap text (evidence-vs-interpretation policy). \
             Found recap text in chunk id={}: {:?}",
            chunk.id,
            &chunk.content[..chunk.content.len().min(200)]
        );
    }

    // The raw prose text MUST appear in at least one chunk.
    let has_user_prose = chunks
        .iter()
        .any(|c| c.content.to_ascii_lowercase().contains("lifetimes"));
    assert!(
        has_user_prose,
        "Expected at least one chunk body to contain raw prose from the turn \
         (keyword 'lifetimes'); found: {:?}",
        chunks
            .iter()
            .map(|c| &c.content[..c.content.len().min(100)])
            .collect::<Vec<_>>()
    );
}

async fn phase2_flush_also_triggers_tree_ingest_inner() {
    let (_tmp, cfg) = test_config_with_tree();
    let (client, provider) = provider_over(&cfg.workspace_dir);
    let hook = hook_with_stubs_and_tree_config(provider.clone(), cfg.clone());

    let session = "phase2-flush-tree";

    // Two turns — no boundary fires, segment stays open.
    for i in 1..=2 {
        hook.on_turn_complete(&TurnContext {
            user_message: format!("Rust borrowing question {i}"),
            assistant_response: format!("Borrowing answer {i}"),
            tool_calls: vec![],
            turn_duration_ms: 50,
            session_id: Some(session.into()),
            agent_id: None,
            entrypoint: None,
            iteration_count: i,
        })
        .await
        .unwrap();
    }

    // Confirm no tree chunks yet (segment still open).
    let before = count_chunks(&cfg).unwrap();
    assert_eq!(
        before, 0,
        "Expected 0 tree chunks before flush; got {before}"
    );

    // Flush should close the segment and trigger tree ingest.
    hook.flush_open_segment(session).await;

    let after = count_chunks(&cfg).unwrap();
    assert!(
        after >= 1,
        "Expected ≥ 1 tree chunk after flush_open_segment triggers segment ingest; got {after}"
    );
}

#[path = "archivist_tests_part_01_tests.rs"]
mod part_01_tests;
