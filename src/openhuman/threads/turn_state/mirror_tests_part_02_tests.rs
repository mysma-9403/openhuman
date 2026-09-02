use super::*;

#[test]
fn subagent_transcript_persists_interleaved_prose_and_tools() {
    let (_d, mut m) = fresh("t");
    m.observe(&AgentProgress::IterationStarted {
        iteration: 1,
        max_iterations: 25,
    });
    m.observe(&AgentProgress::SubagentSpawned {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        mode: "typed".into(),
        dedicated_thread: false,
        prompt_chars: 10,
        prompt: String::new(),
        worker_thread_id: None,
        display_name: Some("Researcher".into()),
    });
    // Reasoning (two same-iteration deltas, must coalesce), then a tool, then
    // visible narration — the order must be preserved in the transcript.
    m.observe(&AgentProgress::SubagentThinkingDelta {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        delta: "let me ".into(),
        iteration: 1,
    });
    m.observe(&AgentProgress::SubagentThinkingDelta {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        delta: "search.".into(),
        iteration: 1,
    });
    // A sub-agent tool boundary must flush the accumulated prose to disk.
    let flushed = m.observe(&AgentProgress::SubagentToolCallStarted {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        call_id: "c1".into(),
        tool_name: "search".into(),
        arguments: serde_json::Value::Null,
        iteration: 1,
        display_label: Some("Searching".into()),
        display_detail: None,
    });
    assert!(flushed, "sub-agent tool boundary must flush");
    m.observe(&AgentProgress::SubagentTextDelta {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        delta: "Found it.".into(),
        iteration: 1,
    });
    m.observe(&AgentProgress::SubagentToolCallCompleted {
        agent_id: "researcher".into(),
        task_id: "sub-1".into(),
        call_id: "c1".into(),
        tool_name: "search".into(),
        success: true,
        output_chars: 5,
        output: "3 hits".into(),
        arguments: None,
        elapsed_ms: 12,
        iteration: 1,
        failure: None,
    });

    let activity = m.snapshot().tool_timeline[0]
        .subagent
        .as_ref()
        .expect("activity")
        .clone();
    assert_eq!(activity.transcript.len(), 3, "thinking, tool, narration");
    match &activity.transcript[0] {
        SubagentTranscriptItem::Thinking { text, .. } => {
            assert_eq!(text, "let me search.", "coalesced same-iteration thinking");
        }
        other => panic!("expected thinking, got {other:?}"),
    }
    match &activity.transcript[1] {
        SubagentTranscriptItem::Tool {
            call_id, status, ..
        } => {
            assert_eq!(call_id, "c1");
            // Completion flips the transcript tool item, not just `tool_calls`.
            assert_eq!(*status, ToolTimelineStatus::Success);
        }
        other => panic!("expected tool, got {other:?}"),
    }
    // The child tool's (capped) result text is persisted on the call so a
    // rehydrated drawer can show what the tool returned.
    assert_eq!(activity.tool_calls[0].output.as_deref(), Some("3 hits"));
    match &activity.transcript[2] {
        SubagentTranscriptItem::Text { text, .. } => assert_eq!(text, "Found it."),
        other => panic!("expected narration, got {other:?}"),
    }

    // The wire form MUST be camelCase — the FE reads `toolName`/`callId`, and
    // snake_case leaking through caused a `replace`-on-undefined crash.
    let json = serde_json::to_string(m.snapshot()).expect("serialize");
    assert!(
        json.contains("\"toolName\""),
        "tool item must serialize camelCase"
    );
    assert!(json.contains("\"callId\""));
    assert!(
        !json.contains("\"tool_name\""),
        "no snake_case fields on the wire"
    );
}

/// When a streaming turn is interrupted and a root transcript already exists,
/// `finish()` appends the partial streamed answer (display-only) to the file.
#[test]
fn finish_appends_interrupted_partial_to_existing_transcript() {
    let dir = tempdir().expect("tempdir");
    let thread_id = "thr_abc";
    let path = seed_root_transcript(dir.path(), thread_id);

    let store = TurnStateStore::new(dir.path().to_path_buf());
    let mut m = TurnStateMirror::new(store, thread_id, "req-9");
    m.observe(&AgentProgress::IterationStarted {
        iteration: 2,
        max_iterations: 25,
    });
    m.observe(&AgentProgress::ThinkingDelta {
        delta: "hmm".into(),
        iteration: 2,
    });
    m.observe(&AgentProgress::TextDelta {
        delta: "half an ".into(),
        iteration: 2,
    });
    m.observe(&AgentProgress::TextDelta {
        delta: "answer".into(),
        iteration: 2,
    });
    // No TurnCompleted — the bridge exits, marking the turn interrupted.
    m.finish();

    // Model context must NOT carry the partial.
    let model = read_transcript(&path).expect("read model context");
    assert!(
        !model
            .messages
            .iter()
            .any(|msg| msg.content.contains("half an answer")),
        "interrupted partial must be excluded from the model context"
    );

    // Display projection carries the flagged partial with request_id + thinking.
    let display = read_transcript_display(&path).expect("read display");
    let partial = display
        .records
        .iter()
        .find_map(|r| match r {
            DisplayRecord::Message(msg) if msg.interrupted => Some(msg),
            _ => None,
        })
        .expect("display must include the interrupted partial");
    assert_eq!(partial.message.content, "half an answer");
    assert_eq!(partial.request_id.as_deref(), Some("req-9"));
    assert_eq!(partial.iteration, Some(2));
    assert_eq!(partial.reasoning_content.as_deref(), Some("hmm"));
}

/// A completed turn never writes an interrupted partial.
#[test]
fn finish_after_completion_writes_no_partial() {
    let dir = tempdir().expect("tempdir");
    let thread_id = "thr_done";
    let path = seed_root_transcript(dir.path(), thread_id);

    let store = TurnStateStore::new(dir.path().to_path_buf());
    let mut m = TurnStateMirror::new(store, thread_id, "req-done");
    m.observe(&AgentProgress::TextDelta {
        delta: "final answer".into(),
        iteration: 1,
    });
    m.observe(&AgentProgress::TurnCompleted { iterations: 1 });
    m.finish();

    let display = read_transcript_display(&path).expect("read display");
    assert!(
        !display
            .records
            .iter()
            .any(|r| matches!(r, DisplayRecord::Message(msg) if msg.interrupted)),
        "a completed turn must not append an interrupted partial"
    );
}

/// An interrupted FIRST turn (no root transcript file yet) is a no-op — the
/// partial stays in the turn_state snapshot only, and finish() does not panic.
#[test]
fn finish_first_turn_without_transcript_is_noop() {
    let dir = tempdir().expect("tempdir");
    let store = TurnStateStore::new(dir.path().to_path_buf());
    let mut m = TurnStateMirror::new(store, "thr_new", "req-first");
    m.observe(&AgentProgress::TextDelta {
        delta: "orphan partial".into(),
        iteration: 1,
    });
    // Must not panic even though no session_raw transcript exists.
    m.finish();
    // The snapshot itself still records the interrupted turn.
    let listed = TurnStateStore::new(dir.path().to_path_buf())
        .get("thr_new")
        .expect("get")
        .expect("snapshot present");
    assert_eq!(listed.lifecycle, TurnLifecycle::Interrupted);
    assert_eq!(listed.streaming_text, "orphan partial");
}
