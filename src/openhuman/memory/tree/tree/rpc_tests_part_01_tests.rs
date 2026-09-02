use super::*;

/// #5169 (`CORE-RUST-1P0`) — a chat batch whose messages omit `timestamp`
/// must ingest, defaulting to `now()`, not reject the whole batch.
///
/// The tolerance lives in `tinycortex` (`ChatMessage::timestamp` carries
/// `#[serde(default = "chrono_now")]`), which is a **separate repository**
/// vendored here as a submodule. Nothing in this repo guarded that
/// contract, so a submodule bump could silently reintroduce the hard
/// rejection and the 4xx-shaped payload would page again. This test is
/// that guard: it fails on the parent-repo side the moment the vendored
/// schema stops tolerating an absent timestamp.
#[test]
fn chat_payload_without_timestamp_is_accepted() {
    let payload = json!({
        "platform": "slack",
        "channel_label": "#general",
        "messages": [{ "author": "alice", "text": "no timestamp here" }],
    });

    let batch: ChatBatch = serde_json::from_value(payload)
        .expect("a chat message omitting `timestamp` must default, not reject the batch");

    assert_eq!(batch.messages.len(), 1);
    assert_eq!(batch.messages[0].text, "no timestamp here");
}

/// Sibling contract for the document arm: `modified_at` is likewise
/// optional (`#[serde(default = "now_utc")]` in tinycortex).
///
/// The payload is deliberately minimal — `title` and `body` are the only
/// required fields on `DocumentInput`. `provider` (`default_provider`),
/// `source_ref` (`Option`) and `modified_at` (`now_utc`) all carry serde
/// defaults, so omitting them together pins the whole optional set rather
/// than just the timestamp.
#[test]
fn document_payload_without_modified_at_is_accepted() {
    let payload = json!({ "title": "Launch plan", "body": "ship it" });

    let doc: DocumentInput = serde_json::from_value(payload)
        .expect("a document omitting `modified_at` must default, not reject");

    assert_eq!(doc.title, "Launch plan");
}

/// Every `SourceKind` reachable from `ingest_rpc` must produce a message
/// the classifier recognises — otherwise that arm's caller errors keep
/// paging while its siblings are demoted, which is the silent-drift
/// failure the enumerated list in `is_invalid_ingest_payload_message`
/// is meant to make impossible to miss.
#[test]
fn all_source_kinds_are_recognised_as_caller_payload_errors() {
    let err = serde_json::from_str::<ChatBatch>("{}").unwrap_err();
    for kind in [SourceKind::Chat, SourceKind::Email, SourceKind::Document] {
        let message = invalid_payload_message(kind, &err);
        assert!(
            is_invalid_ingest_payload_message(&message),
            "{} payload errors must classify as caller errors, got {message:?}",
            kind.as_str()
        );
    }
}

/// The verbatim #5169 message shape, and the negative half: unrelated
/// failures must keep their error severity so real defects still page.
#[test]
fn only_ingest_payload_errors_are_demoted() {
    assert!(is_invalid_ingest_payload_message(
        "invalid chat payload: missing field `timestamp`"
    ));

    for other in [
        "invalid",
        "invalid payload",
        "invalid audio payload: missing field `timestamp`",
        "ingest: chunk store unavailable",
        "chat payload: missing field `timestamp`",
        "something failed: invalid chat payload: missing field `timestamp`",
        "",
    ] {
        assert!(
            !is_invalid_ingest_payload_message(other),
            "{other:?} must keep paging"
        );
    }
}

/// The ingest response is this crate's declaration of a wire the frontend
/// reads, so nothing upstream keeps its keys honest any more.
///
/// It used to be asserted against the engine's own `IngestResult`, on the
/// reasoning that comparing to the upstream type beat hand-writing a key
/// list. That held while the engine produced the body. It does not now:
/// every arm builds from the contract's `IngestOutcome`, so a comparison
/// against the engine summary would pin a shape nothing in this path
/// produces — and it kept the engine linked here purely to describe a wire
/// this crate owns.
///
/// So the expectation is written out. That is the honest form once this
/// crate is the declaring side: the keys below are what the frontend
/// parses, and renaming a field on `IngestResponse` fails here rather than
/// reaching a reader.
#[test]
fn the_response_body_serialises_exactly_as_the_declared_wire() {
    let ours = IngestResponse {
        source_id: "doc-launch".into(),
        chunks_written: 3,
        chunks_dropped: 1,
        chunk_ids: vec!["chunk-a".into(), "chunk-b".into(), "chunk-c".into()],
        extract_jobs_enqueued: 2,
        already_ingested: true,
    };

    assert_eq!(
        serde_json::to_value(&ours).unwrap(),
        serde_json::json!({
            "source_id": "doc-launch",
            "chunks_written": 3,
            "chunks_dropped": 1,
            "chunk_ids": ["chunk-a", "chunk-b", "chunk-c"],
            "extract_jobs_enqueued": 2,
            "already_ingested": true,
        }),
        "the ingest response wire moved — the frontend reads these names"
    );
}

/// Ingest reports what it wrote.
///
/// Bound to the in-process TinyCortex driver rather than left to resolve on
/// its own: the handler asks the driver for the `Ingest` family now, and
/// what a bare test workspace binds is the null driver, which serves none.
/// This is the engine the loadable module wraps, so the counts asserted
/// below are the ones production gets over the bus.
#[tokio::test]
async fn ingest_document_reports_the_chunks_it_wrote() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let outcome = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Document,
            source_id: "doc-launch".into(),
            owner: "alice".into(),
            tags: vec!["launch".into()],
            payload: serde_json::to_value(sample_document(
                "Launch Plan",
                "Phoenix launch canary checklist with rollback steps.",
            ))
            .unwrap(),
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.value.source_id, "doc-launch");
    assert_eq!(outcome.value.chunks_dropped, 0);
    assert!(outcome.value.chunks_written > 0);
    assert!(
        !outcome.value.chunk_ids.is_empty(),
        "the ids are what a caller fetches a chunk back by, so a write \
         that names none is unusable even when the count is right"
    );
}

/// The listing degrades rather than fails when the bound driver has no
/// chunk tier.
///
/// `FixedDiagnostics` is `NullMemoryProvider`-backed, so `as_chunks()` is
/// `None` — the shape of a driver that serves memory without exposing the
/// engine's storage model. The handler is read-only, and an empty page is a
/// true statement about such a driver, so it must not become a
/// caller-facing error. The log still has to report the count it served,
/// because a silent empty and a degraded empty look identical downstream.
#[tokio::test]
async fn list_chunks_reports_empty_when_the_driver_has_no_chunk_tier() {
    let (_tmp, cfg) = test_config();
    bind_diagnostics(&cfg, Default::default(), Default::default());

    let listed = list_chunks_rpc(
        &cfg,
        ListChunksRequest {
            source_kind: Some("document".into()),
            source_id: Some("doc-launch".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .await
    .expect("a driver without the chunk family is not an error");
    assert!(listed.value.chunks.is_empty());
    assert!(listed.logs[0].contains("n=0"), "log: {}", listed.logs[0]);
}

/// The source gate is the driver's, and it survives the move onto the
/// contract: `IngestOutcome::already_ingested` is the field the v1.3.0 pin
/// did not have, and reporting a refused call as a plain empty write is
/// exactly what this test would have started passing over.
#[tokio::test]
async fn ingest_document_is_idempotent_for_duplicate_source_id() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let req = IngestRequest {
        source_kind: SourceKind::Document,
        source_id: "doc-dup".into(),
        owner: "alice".into(),
        tags: vec![],
        payload: serde_json::to_value(sample_document("Launch Plan", "First body")).unwrap(),
    };

    let first = ingest_rpc(&cfg, req.clone()).await.unwrap().value;
    let second = ingest_rpc(&cfg, req).await.unwrap().value;
    assert!(first.chunks_written > 0);
    assert!(!first.already_ingested);
    // `already_ingested` with a zero write count is the whole claim:
    // documents are append-only, so a repeat submission must be recognised
    // rather than duplicated — and told apart from a write that produced
    // nothing, which is the same two numbers with a different cause.
    assert_eq!(second.chunks_written, 0);
    assert!(second.already_ingested);
    assert_eq!(second.source_id, first.source_id);
}

/// Regression #3568 / CORE-2K: chat payloads with RFC-3339 timestamps must
/// be accepted — not rejected with "expected unix timestamp in milliseconds".
#[tokio::test]
async fn ingest_chat_accepts_rfc3339_timestamps() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let outcome = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Chat,
            source_id: "slack:#rfc3339-test".into(),
            owner: "alice".into(),
            tags: vec![],
            payload: json!({
                "platform": "slack",
                "channel_label": "#eng",
                "messages": [
                    {
                        "author": "alice",
                        "timestamp": "2026-05-17T19:30:00Z",
                        "text": "planning the launch"
                    },
                    {
                        "author": "bob",
                        "timestamp": 1779046260000_i64,
                        "text": "confirmed"
                    }
                ]
            }),
        },
    )
    .await
    .unwrap();
    assert!(!outcome.value.chunk_ids.is_empty());
}

/// Regression #3568 / CORE-2K: email payloads with RFC-3339 timestamps must
/// be accepted.
///
/// A driver is bound, like every sibling here. The note this replaces said
/// the mail arm was "still on the in-process pipeline" and that the test
/// would need `install_tinycortex_for_test` "when it moves" — it has moved:
/// the `Email` arm now goes through `ingest_through_driver`, which resolves
/// `provider().as_ingest()` and refuses a driver that does not serve it.
/// Without the binding the test only passed because CI happens to set
/// `TINYMEMORY_TEST_MODULE` to a module that serves `Ingest`, so it would
/// fail on a machine that does not.
#[tokio::test]
async fn ingest_email_accepts_rfc3339_timestamps() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let outcome = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Email,
            source_id: "gmail:rfc3339-test".into(),
            owner: "alice@example.com".into(),
            tags: vec![],
            payload: json!({
                "provider": "gmail",
                "thread_subject": "Launch",
                "messages": [
                    {
                        "from": "bob@example.com",
                        "to": ["alice@example.com"],
                        "subject": "Launch",
                        "sent_at": "2026-05-17T19:30:00Z",
                        "body": "Let's ship this."
                    }
                ]
            }),
        },
    )
    .await
    .unwrap();
    assert!(!outcome.value.chunk_ids.is_empty());
}

/// One empty message must not fail the batch around it.
///
/// `validate_ingest_item` answers `Invalid` for content that trims to
/// empty, and the driver validates every item before ingesting any — so an
/// attachment-only message, which reaches this handler as a message with no
/// text, would turn a batch that has real content in it into a failed call.
/// The in-process pipeline wrote the rest of the batch and rendered that
/// message as a bare header; the filter keeps the first half of that and
/// gives up only the header.
#[tokio::test]
async fn an_empty_chat_message_does_not_fail_the_batch_around_it() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let outcome = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Chat,
            source_id: "slack:#attachment-only".into(),
            owner: "alice".into(),
            tags: vec![],
            payload: json!({
                "platform": "slack",
                "channel_label": "#eng",
                "messages": [
                    {
                        "author": "alice",
                        "timestamp": "2026-05-17T19:30:00Z",
                        "text": "   "
                    },
                    {
                        "author": "bob",
                        "timestamp": "2026-05-17T19:31:00Z",
                        "text": "here is the plan"
                    }
                ]
            }),
        },
    )
    .await
    .expect("an empty message is dropped, not a batch failure");
    assert!(
        !outcome.value.chunk_ids.is_empty(),
        "the surviving message must still be written"
    );
}

/// An ingest is a write, so a driver without the family is refused rather
/// than answered with zeros.
///
/// The counts have no way to say "nothing was handed over": zero written
/// and zero dropped is what a successful ingest of nothing looks like too,
/// so degrading here would report content dropped on the floor as a
/// success. `FixedDiagnostics` advertises `Capabilities::all()` while
/// serving no `Ingest` accessor, which also pins that the refusal keys off
/// the accessor and not off the advertised set.
#[tokio::test]
async fn ingest_refuses_a_driver_that_does_not_serve_the_ingest_family() {
    let (_tmp, cfg) = test_config();
    bind_diagnostics(&cfg, Default::default(), Default::default());

    let err = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Chat,
            source_id: "slack:#no-ingest".into(),
            owner: "alice".into(),
            tags: vec![],
            payload: json!({
                "platform": "slack",
                "channel_label": "#eng",
                "messages": [{ "author": "alice", "text": "anything at all" }],
            }),
        },
    )
    .await
    .unwrap_err();
    assert!(
        err.contains("does not serve Ingest"),
        "the refusal must name the missing family: {err}"
    );
    assert!(
        err.contains("fixed-diagnostics"),
        "the refusal must name the driver that refused: {err}"
    );
}

#[tokio::test]
async fn ingest_rpc_rejects_invalid_document_payload() {
    let (_tmp, cfg) = test_config();
    let err = ingest_rpc(
        &cfg,
        IngestRequest {
            source_kind: SourceKind::Document,
            source_id: "doc-invalid".into(),
            owner: String::new(),
            tags: vec![],
            payload: json!({"title": "Missing body"}),
        },
    )
    .await
    .unwrap_err();
    assert!(err.contains("invalid document payload"));
}

#[tokio::test]
async fn list_chunks_rejects_unknown_source_kind() {
    let (_tmp, cfg) = test_config();
    let err = list_chunks_rpc(
        &cfg,
        ListChunksRequest {
            source_kind: Some("nonsense".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(err.contains("unknown source kind: nonsense"));
}

/// An id the driver cannot resolve is `Ok(None)`, never an error — and the
/// same is true of a driver with no chunk tier at all, which is why one
/// test covers both. The two cases are indistinguishable to a caller by
/// design: "no such chunk" is the honest answer to either.
#[tokio::test]
async fn get_chunk_returns_none_for_missing_id() {
    let (_tmp, cfg) = test_config();
    bind_diagnostics(&cfg, Default::default(), Default::default());
    let outcome = get_chunk_rpc(
        &cfg,
        GetChunkRequest {
            id: "missing-chunk".into(),
        },
    )
    .await
    .unwrap();
    assert!(outcome.value.chunk.is_none());
}

/// #1574 §4b: `backfill_status_rpc` reports what the driver says is
/// queued for the backfill kind, and a non-zero count forces
/// `in_progress` so the modal stays open.
///
/// The empty case now asserts `in_progress` too. It could not before: the
/// flag was a process-global that parallel tests shared. It comes from the
/// bound driver now, so it is this test's to set.
///
/// Ready + running, and deliberately not `total - done`: a backfill job
/// that failed is finished with, and counting it as pending would leave
/// the modal open forever.
#[tokio::test]
async fn backfill_status_reports_the_drivers_pending_count() {
    use crate::openhuman::memory::api::provider::types::QueueStats;

    let (_tmp, cfg) = test_config();

    bind_diagnostics(&cfg, Default::default(), QueueStats::default());
    let s0 = backfill_status_rpc(&cfg).await.unwrap().value;
    assert_eq!(s0.pending_jobs, 0, "idle space has no pending backfill");

    bind_diagnostics(
        &cfg,
        Default::default(),
        QueueStats {
            ready: 1,
            running: 2,
            // Neither of these is pending work.
            done: 7,
            failed: 3,
            ..Default::default()
        },
    );
    let s1 = backfill_status_rpc(&cfg).await.unwrap().value;
    assert_eq!(
        s1.pending_jobs, 3,
        "ready + running is what is still to do; done and failed are not"
    );
    assert!(s1.in_progress, "pending>0 forces in_progress=true");
}

/// The backfill flag is the driver's answer, not the host's engine static.
///
/// This is the gap the counts cannot express: a backfill chain re-enqueues
/// itself, so between one link settling and the next being written there is
/// an instant with nothing ready, nothing running, and the work unfinished.
/// A poll that trusted the counts alone closes the re-embed modal there.
///
/// It has to come from the driver rather than
/// `tinymemory_core::queue::backfill_in_progress()`, because re-embedding
/// runs in the module and a `cdylib` has its own statics — the host-linked
/// copy reads `false` forever on that path, which is worse than coarse.
#[tokio::test]
async fn backfill_status_reports_the_drivers_flag_when_the_counts_are_empty() {
    use crate::openhuman::memory::api::provider::types::QueueStats;

    let (_tmp, cfg) = test_config();

    let driver = std::sync::Arc::new(
        crate::openhuman::memory::binding::FixedDiagnostics::new(
            Default::default(),
            QueueStats::default(),
        )
        .backfilling(),
    );
    crate::openhuman::memory::binding::install_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        driver as std::sync::Arc<dyn crate::openhuman::memory::api::provider::MemoryProvider>,
    );

    let status = backfill_status_rpc(&cfg).await.unwrap().value;
    assert_eq!(
        status.pending_jobs, 0,
        "precondition: the counts say the queue is empty"
    );
    assert!(
        status.in_progress,
        "and the driver still says a backfill is running, which is the whole point"
    );
}

// ── pipeline_status / set_enabled (#1856 Part 1) ─────────────────────

/// `derive_pipeline_status` precedence is locked in here so the UI can
/// rely on the wire status string without re-deriving it from the raw
/// counters.
#[test]
fn latest_quarantine_reads_the_newest_copy_and_derives_resynced() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("memory_tree");
    std::fs::create_dir_all(&dir).unwrap();
    // No quarantine file: nothing to report.
    assert!(latest_quarantine(tmp.path(), 0).is_none());

    std::fs::write(dir.join("chunks.db.corrupt-20260101T000000Z"), b"old").unwrap();
    std::fs::write(dir.join("chunks.db.corrupt-20260827T070304Z"), b"new").unwrap();
    // Side files never match the main-file prefix.
    std::fs::write(dir.join("chunks.db-wal.corrupt-20261231T235959Z"), b"wal").unwrap();
    // Garbage that starts with the prefix but has no parsable stamp is ignored.
    std::fs::write(dir.join("chunks.db.corrupt-notastamp"), b"x").unwrap();

    let at = chrono::NaiveDate::from_ymd_opt(2026, 8, 27)
        .unwrap()
        .and_hms_opt(7, 3, 4)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    // The rebuilt store is still empty: the notice stands.
    let pending = latest_quarantine(tmp.path(), 0).expect("newest quarantine");
    assert_eq!(pending.quarantined_at_ms, at);
    assert!(pending
        .quarantined_path
        .ends_with("chunks.db.corrupt-20260827T070304Z"));
    assert!(!pending.resynced);

    // Any chunk in the rebuilt store: the user re-synced, the notice retires.
    // Chunk *content* time is irrelevant here: restored history predates the
    // quarantine forever, which is exactly why this is not a timestamp test.
    let done = latest_quarantine(tmp.path(), 1).expect("newest quarantine");
    assert!(done.resynced);
}
