use super::*;

#[tokio::test]
async fn archivist_indexes_turn() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    let ctx = TurnContext {
        user_message: "What is Rust?".into(),
        assistant_response: "Rust is a systems programming language.".into(),
        tool_calls: vec![],
        turn_duration_ms: 500,
        session_id: Some("test-session".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    };

    hook.on_turn_complete(&ctx).await.unwrap();

    let entries = fts5::episodic_session_entries(&conn, "test-session").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].role, "user");
    assert_eq!(entries[1].role, "assistant");
}

#[tokio::test]
async fn archivist_creates_segment_on_first_turn() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    let ctx = TurnContext {
        user_message: "Hello world".into(),
        assistant_response: "Hi there!".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some("seg-test".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    };

    hook.on_turn_complete(&ctx).await.unwrap();

    let open = seg::open_segment_for_session(&conn, "seg-test").unwrap();
    assert!(open.is_some());
    assert_eq!(open.unwrap().turn_count, 1);
}

#[tokio::test]
async fn archivist_detects_topic_change_boundary() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    hook.on_turn_complete(&TurnContext {
        user_message: "Tell me about Rust".into(),
        assistant_response: "Rust is great.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some("boundary-test".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    hook.on_turn_complete(&TurnContext {
        user_message: "How about its memory safety?".into(),
        assistant_response: "It uses ownership.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some("boundary-test".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    })
    .await
    .unwrap();

    hook.on_turn_complete(&TurnContext {
        user_message: "Switching to a different topic now. I prefer dark mode.".into(),
        assistant_response: "Noted about dark mode.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some("boundary-test".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 3,
    })
    .await
    .unwrap();

    let segments = seg::segments_by_namespace(&conn, "global", 10).unwrap();
    assert!(
        segments.len() >= 2,
        "Expected at least 2 segments, got {}",
        segments.len()
    );
}

#[tokio::test]
async fn archivist_extracts_failure_lesson() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    let ctx = TurnContext {
        user_message: "Run tests".into(),
        assistant_response: "Tests failed.".into(),
        tool_calls: vec![ToolCallRecord {
            name: "shell".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
            success: false,
            output_summary: "shell: failed (error)".into(),
            duration_ms: 3000,
        }],
        turn_duration_ms: 3500,
        session_id: Some("test-session-2".into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    };

    hook.on_turn_complete(&ctx).await.unwrap();

    let entries = fts5::episodic_session_entries(&conn, "test-session-2").unwrap();
    let assistant_entry = entries.iter().find(|e| e.role == "assistant").unwrap();
    assert!(assistant_entry.lesson.as_ref().unwrap().contains("shell"));
}

#[tokio::test]
async fn disabled_archivist_is_noop() {
    let hook = ArchivistHook::disabled();
    let ctx = TurnContext {
        user_message: "test".into(),
        assistant_response: "test".into(),
        tool_calls: vec![],
        turn_duration_ms: 0,
        session_id: None,
        agent_id: None,
        entrypoint: None,
        iteration_count: 0,
    };
    hook.on_turn_complete(&ctx).await.unwrap();
}

#[test]
fn extract_profile_key_works() {
    let key = extract_profile_key("I prefer dark mode for coding", "preference");
    assert!(key.starts_with("preference_"));
    assert!(key.contains("prefer"));
}

#[tokio::test]
async fn archivist_accumulates_turns_in_segment() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    let session = "accum-session";

    for i in 1..=3 {
        hook.on_turn_complete(&TurnContext {
            user_message: format!("Turn number {i}"),
            assistant_response: format!("Response {i}"),
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

    let open_seg = seg::open_segment_for_session(&conn, session)
        .unwrap()
        .expect("Expected an open segment after 3 turns");

    assert_eq!(
        open_seg.turn_count, 3,
        "Segment should have accumulated 3 turns, got {}",
        open_seg.turn_count
    );
}

#[tokio::test]
async fn archivist_extracts_preference_event_on_boundary() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = ArchivistHook::new(provider.clone(), true);

    let session = "pref-boundary-session";

    hook.on_turn_complete(&TurnContext {
        user_message: "Tell me about Rust ownership".into(),
        assistant_response: "Ownership is a key concept in Rust.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    hook.on_turn_complete(&TurnContext {
        user_message: "I prefer dark mode for all my editors".into(),
        assistant_response: "Good to know! Dark mode is easier on the eyes.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    })
    .await
    .unwrap();

    hook.on_turn_complete(&TurnContext {
        user_message: "Switching to a different topic — how does Tokio work?".into(),
        assistant_response: "Tokio is an async runtime.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 3,
    })
    .await
    .unwrap();

    let events = ev::events_by_type(&conn, "global", "preference", 20).unwrap();
    assert!(
        !events.is_empty(),
        "Expected at least one preference event after segment close; got 0."
    );
    let has_dark_mode = events
        .iter()
        .any(|e| e.content.to_lowercase().contains("prefer"));
    assert!(
        has_dark_mode,
        "Expected a preference event mentioning 'prefer', found: {:?}",
        events.iter().map(|e| &e.content).collect::<Vec<_>>()
    );
}

// ── Phase 0: episodic_capture_enabled independent of learning.enabled ────────

/// When `learning.enabled = false` but `episodic_capture_enabled = true`,
/// the ArchivistHook (constructed directly, as builder.rs would produce)
/// must still write 2 episodic_log rows (user + assistant) and create/advance
/// a segment. This verifies the core contract: episodic capture runs
/// regardless of the learning inference stack toggle.
#[tokio::test]
async fn phase0_episodic_rows_and_segment_without_learning_enabled() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    // Simulate what builder.rs does when learning.enabled=false but
    // episodic_capture_enabled=true: construct the hook directly with
    // the SQLite conn, enabled=true. No config attached (no LLM recap
    // or tree ingest — those are gated by learning.enabled / chat_to_tree_enabled).
    let hook = ArchivistHook::new(provider.clone(), true);

    let session = "phase0-test-session";

    hook.on_turn_complete(&TurnContext {
        user_message: "Hello, what is Rust?".into(),
        assistant_response: "Rust is a systems language.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    // Verify 2 episodic rows were written.
    let entries = fts5::episodic_session_entries(&conn, session).unwrap();
    assert_eq!(
        entries.len(),
        2,
        "Expected 2 episodic rows (user + assistant), got {}",
        entries.len()
    );
    assert_eq!(entries[0].role, "user");
    assert_eq!(entries[1].role, "assistant");

    // Verify a segment was created.
    let open_seg = seg::open_segment_for_session(&conn, session)
        .unwrap()
        .expect("Expected an open segment after first turn");
    assert_eq!(open_seg.turn_count, 1);

    // Add a second turn to verify segment advances.
    hook.on_turn_complete(&TurnContext {
        user_message: "Tell me more about ownership.".into(),
        assistant_response: "Ownership prevents data races.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    })
    .await
    .unwrap();

    let entries2 = fts5::episodic_session_entries(&conn, session).unwrap();
    assert_eq!(
        entries2.len(),
        4,
        "Expected 4 episodic rows after 2 turns, got {}",
        entries2.len()
    );
    let open_seg2 = seg::open_segment_for_session(&conn, session)
        .unwrap()
        .expect("Expected an open segment after 2 turns");
    assert_eq!(
        open_seg2.turn_count, 2,
        "Segment should have 2 turns, got {}",
        open_seg2.turn_count
    );
}

/// When a segment closes, the LLM chat provider recap is used (verified by
/// a non-empty segment summary) and an embedding row is written to
/// `segment_embeddings`.
#[tokio::test]
async fn phase1_llm_recap_and_embedding_on_segment_close() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = hook_with_stubs(provider.clone());

    let session = "phase1-recap-test";

    // Turn 1 — opens first segment.
    hook.on_turn_complete(&TurnContext {
        user_message: "Tell me about Rust ownership".into(),
        assistant_response: "Rust's ownership model prevents data races.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 1,
    })
    .await
    .unwrap();

    // Turn 2 — continues same segment.
    hook.on_turn_complete(&TurnContext {
        user_message: "What about the borrow checker?".into(),
        assistant_response: "The borrow checker enforces ownership rules at compile time.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 2,
    })
    .await
    .unwrap();

    // Turn 3 — topic change triggers a boundary → closes first segment → recap + embed fire.
    hook.on_turn_complete(&TurnContext {
        user_message: "Completely different topic: what is async/await in Python?".into(),
        assistant_response: "Python asyncio enables concurrent programming.".into(),
        tool_calls: vec![],
        turn_duration_ms: 100,
        session_id: Some(session.into()),
        agent_id: None,
        entrypoint: None,
        iteration_count: 3,
    })
    .await
    .unwrap();

    // Verify segments exist.
    let segments = seg::segments_by_namespace(&conn, "global", 10).unwrap();
    assert!(
        segments.len() >= 2,
        "Expected at least 2 segments (closed + open), got {}",
        segments.len()
    );

    // Find the closed segment (has a summary).
    let closed = segments
        .iter()
        .find(|s| s.summary.as_ref().map(|s| !s.is_empty()).unwrap_or(false));
    assert!(
        closed.is_some(),
        "Expected at least one closed segment with a non-empty summary"
    );

    let closed_seg = closed.unwrap();
    let summary = closed_seg.summary.as_ref().unwrap();
    // The stub provider returns a fixed string — verify it was persisted.
    assert!(
        summary.contains("stub recap"),
        "Expected summary to contain 'stub recap', got: {:?}",
        summary
    );
}

/// `flush_open_segment` must force-close the trailing open segment and
/// trigger recap + embedding even without a boundary-triggering turn.
#[tokio::test]
async fn phase1_flush_open_segment_finalizes_trailing_segment() {
    let (_tmp, client, provider) = setup_provider();
    let conn = client.profile_conn();
    let hook = hook_with_stubs(provider.clone());

    let session = "phase1-flush-test";

    // Write 2 turns — stays in one open segment (no topic boundary fires).
    for i in 1..=2 {
        hook.on_turn_complete(&TurnContext {
            user_message: format!("Question about Rust turn {i}"),
            assistant_response: format!("Answer about Rust turn {i}"),
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

    // Confirm the segment is still open (no boundary fired).
    let open_seg_before = seg::open_segment_for_session(&conn, session).unwrap();
    assert!(
        open_seg_before.is_some(),
        "Expected an open segment before flush"
    );

    // Flush — should force-close, recap, and embed.
    hook.flush_open_segment(session).await;

    // Segment should now be closed (no open segment for this session).
    let open_seg_after = seg::open_segment_for_session(&conn, session).unwrap();
    assert!(
        open_seg_after.is_none(),
        "Expected no open segment after flush_open_segment"
    );

    // The formerly-open segment should now have a summary.
    let segments = seg::segments_by_namespace(&conn, "global", 10).unwrap();
    let flushed = segments.iter().find(|s| {
        s.session_id == session && s.summary.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
    });
    assert!(
        flushed.is_some(),
        "Expected flushed segment to have a non-empty summary"
    );
}

/// After a single turn (no segment boundary), the tree must have ZERO chunks —
/// the per-turn pipe_turn_to_tree path no longer exists.
#[tokio::test]
async fn phase2_no_per_turn_tree_write() {
    with_stub_chat_provider(phase2_no_per_turn_tree_write_inner()).await
}

/// When a segment closes (boundary triggered), exactly ONE tree ingest fires
/// for that segment containing all its turns — not one ingest per turn.
#[tokio::test]
async fn phase2_exactly_one_tree_ingest_per_segment_close() {
    with_stub_chat_provider(phase2_exactly_one_tree_ingest_per_segment_close_inner()).await
}

/// The ingested leaf messages must carry the episodic-provenance `source_ref`
/// in the expected format:
/// `agent://session/{session_id}/segment/{segment_id}#ep{start}-{end}`.
///
/// Also verifies that `source_id` is the constant `"conversations:agent"`.
#[tokio::test]
async fn phase2_provenance_stamped_on_leaf_and_source_id_is_constant() {
    with_stub_chat_provider(phase2_provenance_stamped_on_leaf_and_source_id_is_constant_inner())
        .await
}

/// The ingested content must be the raw prose turns (user + assistant text),
/// NOT equal to the LLM recap text. The recap lives only in the STM segment
/// layer; the tree must ingest raw evidence so it can build its own summaries.
#[tokio::test]
async fn phase2_ingested_content_is_raw_prose_not_recap() {
    with_stub_chat_provider(phase2_ingested_content_is_raw_prose_not_recap_inner()).await
}

/// `flush_open_segment` must also trigger the tree ingest for the trailing
/// open segment (same as on_segment_closed at a topic boundary).
#[tokio::test]
async fn phase2_flush_also_triggers_tree_ingest() {
    with_stub_chat_provider(phase2_flush_also_triggers_tree_ingest_inner()).await
}

// ── #13021 empty/whitespace recap embed-skip guard ───────────────────────────
//
// `on_segment_closed` defends against ever passing an empty or whitespace
// recap into the embedder by calling `embed_segment_recap`, which short-
// circuits before `Embedder::embed` runs. The skip is unreachable through
// the current `summarize_entries` call graph today (the heuristic
// `fallback_summary` always returns non-empty text), so these tests drive
// `embed_segment_recap` directly to lock the guard against future
// regressions where `summarize_entries` could return `""`.

/// An empty recap must short-circuit before any scoring call.
///
/// Uses `RecordingProvider` so we can assert that `scoring.embed_text` is
/// absent — confirming the guard fired before the scoring path, not merely
/// that no row landed in a DB queried with the wrong model_signature key.
#[tokio::test]
async fn embed_segment_recap_skips_empty_summary() {
    use crate::openhuman::memory::guard::test_support::RecordingProvider;
    let recording = Arc::new(RecordingProvider::new());
    let provider: Arc<dyn MemoryProvider> = recording.clone();
    let hook = hook_with_stubs(provider);

    hook.embed_segment_recap("seg-empty-recap", "", 3.0).await;

    let calls = recording.calls();
    let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
    assert!(
        !methods.contains(&"scoring.embed_text"),
        "scoring.embed_text must not be called for an empty recap; got {methods:?}"
    );
}

/// Whitespace-only recaps (newlines, tabs, spaces) must also short-circuit
/// — the upstream provider rejects whitespace inputs the same way it
/// rejects empty inputs (#13021).
///
/// Uses `RecordingProvider` so we can assert that `scoring.embed_text` is
/// absent — confirming the guard fired before the scoring path, not merely
/// that no row landed in a DB queried with the wrong model_signature key.
#[tokio::test]
async fn embed_segment_recap_skips_whitespace_summary() {
    use crate::openhuman::memory::guard::test_support::RecordingProvider;
    let recording = Arc::new(RecordingProvider::new());
    let provider: Arc<dyn MemoryProvider> = recording.clone();
    let hook = hook_with_stubs(provider);

    hook.embed_segment_recap("seg-ws-recap", "   \n\t  ", 3.0)
        .await;

    let calls = recording.calls();
    let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
    assert!(
        !methods.contains(&"scoring.embed_text"),
        "scoring.embed_text must not be called for a whitespace-only recap; got {methods:?}"
    );
}

/// Positive control: a non-empty recap must reach `embedder_slug` then
/// `embed_text` on the scoring family, in that order, and pass the recap text
/// verbatim to `embed_text`.
#[tokio::test]
async fn embed_segment_recap_reaches_scoring_for_non_empty_summary() {
    use crate::openhuman::memory::guard::test_support::RecordingProvider;
    let recording = Arc::new(RecordingProvider::new());
    let provider: Arc<dyn MemoryProvider> = recording.clone();
    let hook = hook_with_stubs(provider);

    hook.embed_segment_recap("seg-ok-recap", "real recap text", 3.0)
        .await;

    let calls = recording.calls();
    let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();

    let slug_pos = methods
        .iter()
        .position(|&m| m == "scoring.embedder_slug")
        .unwrap_or_else(|| panic!("scoring.embedder_slug must be called; got {methods:?}"));
    let embed_pos = methods
        .iter()
        .position(|&m| m == "scoring.embed_text")
        .expect("scoring.embed_text must be called for a non-empty recap");

    assert!(
        slug_pos < embed_pos,
        "scoring.embedder_slug ({slug_pos}) must be called before scoring.embed_text ({embed_pos})"
    );
    assert_eq!(
        calls[embed_pos].content.as_deref(),
        Some("real recap text"),
        "embed_text must receive the recap text verbatim"
    );
}
