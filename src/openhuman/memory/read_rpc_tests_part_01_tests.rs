use super::*;

#[tokio::test]
async fn list_chunks_returns_seeded_chunk() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "hello @alice phoenix migration").await;
    let resp = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    assert!(!resp.chunks.is_empty());
    assert_eq!(resp.total, resp.chunks.len() as u64);
}

#[tokio::test]
async fn list_chunks_filters_by_source_id() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alpha").await;
    seed_chat_chunk(&cfg, "slack:#b", "beta").await;
    let only_a = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_ids: Some(vec!["slack:#a".into()]),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert!(only_a.chunks.iter().all(|c| c.source_id == "slack:#a"));
    assert!(only_a.total >= 1);
}

#[tokio::test]
async fn list_chunks_query_substring_works() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "phoenix migration ships friday").await;
    seed_chat_chunk(&cfg, "slack:#eng", "different unrelated text").await;
    let resp = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            query: Some("phoenix".into()),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert!(resp.chunks.iter().any(|c| {
        c.content_preview
            .as_deref()
            .unwrap_or("")
            .contains("phoenix")
    }));
}

#[tokio::test]
async fn list_chunks_filters_by_source_kind_and_applies_limit_offset() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "first chat").await;
    seed_chat_chunk(&cfg, "slack:#b", "second chat").await;

    let filtered = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_kinds: Some(vec!["chat".into()]),
            limit: Some(1),
            offset: Some(1),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(filtered.chunks.len(), 1);
    assert_eq!(filtered.total, 2);
    assert!(filtered.chunks.iter().all(|c| c.source_kind == "chat"));
}

#[tokio::test]
async fn list_chunks_filters_by_entity_id_and_time_window() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com handles phoenix").await;
    seed_chat_chunk(&cfg, "slack:#eng", "bob@example.com handles atlas").await;

    let seeded = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let alice = seeded
        .iter()
        .find(|chunk| {
            chunk
                .content_preview
                .as_deref()
                .unwrap_or("")
                .contains("alice@example.com")
        })
        .expect("alice chunk present");
    let bob = seeded
        .iter()
        .find(|chunk| {
            chunk
                .content_preview
                .as_deref()
                .unwrap_or("")
                .contains("bob@example.com")
        })
        .expect("bob chunk present");

    update_chunk_timestamp(&cfg, &alice.id, 1_700_000_000_100);
    update_chunk_timestamp(&cfg, &bob.id, 1_700_000_000_900);

    let filtered = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            entity_ids: Some(vec!["email:alice@example.com".into()]),
            since_ms: Some(1_700_000_000_000),
            until_ms: Some(1_700_000_000_500),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;

    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.chunks.len(), 1);
    assert_eq!(filtered.chunks[0].id, alice.id);
}

#[tokio::test]
async fn list_chunks_ignores_empty_filter_lists_and_blank_query() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alpha").await;
    seed_chat_chunk(&cfg, "slack:#b", "beta").await;

    let resp = list_chunks_rpc(
        &cfg,
        ChunkFilter {
            source_kinds: Some(vec![]),
            source_ids: Some(vec![]),
            entity_ids: Some(vec![]),
            query: Some("   ".into()),
            limit: Some(10),
            ..ChunkFilter::default()
        },
    )
    .await
    .unwrap()
    .value;

    assert_eq!(resp.total, 2);
    assert_eq!(resp.chunks.len(), 2);
}

#[tokio::test]
async fn list_chunks_normalizes_invalid_tags_negative_tokens_and_empty_content() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_raw_chunk(
        &cfg,
        "raw-empty",
        "document",
        "notion:page-1",
        1_700_000_000_123,
        "not-json",
        "",
        -7,
    );

    let resp = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    let row = resp
        .chunks
        .into_iter()
        .find(|chunk| chunk.id == "raw-empty")
        .expect("raw chunk listed");

    assert_eq!(row.token_count, 0);
    assert_eq!(row.tags, Vec::<String>::new());
    assert_eq!(row.content_preview, None);
    assert!(!row.has_embedding);
}

#[tokio::test]
async fn list_sources_aggregates() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "x").await;
    seed_chat_chunk(&cfg, "slack:#a", "y").await;
    seed_chat_chunk(&cfg, "slack:#b", "z").await;
    let sources = list_sources_rpc(&cfg, None).await.unwrap().value;
    let a = sources
        .iter()
        .find(|s| s.source_id == "slack:#a")
        .expect("expected slack:#a");
    let b = sources
        .iter()
        .find(|s| s.source_id == "slack:#b")
        .expect("expected slack:#b");
    assert_eq!(a.chunk_count, 2);
    assert_eq!(b.chunk_count, 1);
}

#[tokio::test]
async fn list_sources_formats_email_threads_with_trimmed_user_hint() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    insert_raw_chunk(
        &cfg,
        "email-thread",
        "email",
        "gmail:Alice@Example.com|bob@example.com|carol@example.com",
        1_700_000_000_123,
        "[]",
        "thread body",
        12,
    );

    let sources = list_sources_rpc(&cfg, Some(" alice@example.com ".into()))
        .await
        .unwrap()
        .value;
    let source = sources
        .iter()
        .find(|row| row.source_id == "gmail:Alice@Example.com|bob@example.com|carol@example.com")
        .expect("email thread source present");
    assert_eq!(source.display_name, "bob@example.com, carol@example.com");
}

#[tokio::test]
async fn entity_index_for_returns_extracted_entities() {
    let (_tmp, cfg) = test_config();
    // The entity index is read through `MemoryEntities::chunk_entities`, so the
    // handler needs a driver that serves that family — the null fallback does not.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    // Find the chunk we just seeded.
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = &chunks[0].id;
    let refs = entity_index_for_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(
        refs.iter().any(|r| r.entity_id.contains("alice")),
        "expected alice entity in index, got: {refs:?}"
    );
}

#[tokio::test]
async fn chunks_for_entity_returns_leaf_chunk_ids_only() {
    let (_tmp, cfg) = test_config();
    // `MemoryEntities::entity_chunk_ids` answers this one; the null fallback
    // serves no entity tier and would report an empty list.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let chunk_id = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks[0]
        .id
        .clone();

    let rows = chunks_for_entity_rpc(&cfg, "email:alice@example.com".into())
        .await
        .unwrap()
        .value;
    assert_eq!(rows, vec![chunk_id]);
}

#[tokio::test]
async fn top_entities_returns_most_frequent() {
    let (_tmp, cfg) = test_config();
    // `MemoryEntities::top_entities` answers this one; the null fallback
    // serves no entity tier and would report an empty ranking.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#a", "alice@example.com x").await;
    seed_chat_chunk(&cfg, "slack:#b", "alice@example.com y").await;
    seed_chat_chunk(&cfg, "slack:#c", "bob@example.com z").await;
    let top = top_entities_rpc(&cfg, Some("email".into()), 10)
        .await
        .unwrap()
        .value;
    assert!(top
        .iter()
        .any(|e| e.entity_id == "email:alice@example.com" && e.count >= 2));
}

#[tokio::test]
async fn delete_chunk_removes_chunk_and_dependent_rows() {
    let (_tmp, cfg) = test_config();
    // The delete goes through `MemorySourceSink::forget_matching`, which the null
    // fallback does not serve — and this handler refuses rather than degrades.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = chunks[0].id.clone();
    let resp = delete_chunk_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(resp.deleted);
    // Re-list — the chunk should be gone.
    let after = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value;
    assert!(after.chunks.iter().all(|c| c.id != id));
}

#[tokio::test]
async fn delete_missing_chunk_is_idempotent() {
    let (_tmp, cfg) = test_config();
    // Idempotence is the driver's, so this needs a driver: without one the handler
    // refuses outright, which is a different answer from "that chunk was not there".
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    let resp = delete_chunk_rpc(&cfg, "does-not-exist".into())
        .await
        .unwrap()
        .value;
    assert!(!resp.deleted);
    assert_eq!(resp.score_rows_removed, 0);
}

/// The one named behaviour delta of the move onto `MemoryEntities::top_entities`,
/// pinned in both directions.
///
/// The member validates `kind` and answers `MemoryError::Invalid` for one it does
/// not recognise. The SQL this handler used to run compared the string against the
/// stored column, so an unknown kind matched nothing and the caller got an empty
/// list. A migration must not turn that quiet empty result into a user-visible
/// error, so the variant is mapped back — and the second half of this test is what
/// keeps the map-back narrow rather than a blanket swallow.
#[tokio::test]
async fn top_entities_reports_empty_for_an_unknown_kind() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;

    let unknown = top_entities_rpc(&cfg, Some("not-a-kind".into()), 10)
        .await
        .expect("an unrecognised kind is an empty ranking, not an error")
        .value;
    assert!(unknown.is_empty(), "got: {unknown:?}");

    let known = top_entities_rpc(&cfg, Some("email".into()), 10)
        .await
        .unwrap()
        .value;
    assert!(
        !known.is_empty(),
        "a recognised kind must still rank rows; the Invalid map-back is narrow"
    );
}

/// `ForgetOutcome` reports `chunks_removed` and `trees_cleaned` and nothing about
/// the per-chunk side rows, so `DeleteChunkResponse`'s two counts are observed
/// before the delete rather than read off the outcome. This pins that they are
/// still real numbers — dropping them to zero would read as "there was nothing to
/// clean up", which is a different claim from "nobody counted".
#[tokio::test]
async fn delete_chunk_still_reports_its_side_row_counts() {
    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "alice@example.com owns it").await;
    let id = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks[0]
        .id
        .clone();

    let indexed = entity_index_for_rpc(&cfg, id.clone()).await.unwrap().value;
    let indexed_rows: u32 = indexed.iter().map(|entity| entity.count).sum();
    assert!(
        indexed_rows > 0,
        "expected entity-index rows, got: {indexed:?}"
    );

    let resp = delete_chunk_rpc(&cfg, id).await.unwrap().value;
    assert!(resp.deleted);
    assert_eq!(resp.entity_index_rows_removed, indexed_rows);
    assert_eq!(resp.score_rows_removed, 1, "ingest writes one score row");
}

#[tokio::test]
async fn chunk_score_returns_breakdown_after_ingest() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(
        &cfg,
        "slack:#eng",
        "alice@example.com owns the phoenix migration",
    )
    .await;
    let chunks = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks;
    let id = &chunks[0].id;
    let breakdown = chunk_score_rpc(&cfg, id.clone()).await.unwrap().value;
    assert!(breakdown.is_some(), "expected score row after ingest");
    let b = breakdown.unwrap();
    assert!(b.signals.iter().any(|s| s.name == "metadata_weight"));
    assert!(b.threshold > 0.0);
}

#[tokio::test]
async fn search_returns_matching_chunks() {
    let (_tmp, cfg) = test_config();
    // These handlers read through the bound driver now that the raw SQL is
    // gone, so the test has to bind one. TinyCortex is the engine the
    // loadable module wraps, so this exercises the same code production
    // reaches over the bus — and unlike the module it is not a process
    // singleton, which is what lets these run in one test binary.
    crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
    seed_chat_chunk(&cfg, "slack:#eng", "phoenix migration scheduled friday").await;
    seed_chat_chunk(&cfg, "slack:#eng", "different unrelated text").await;
    let hits = search_rpc(&cfg, "phoenix".into(), 10).await.unwrap().value;
    assert!(hits.iter().any(|c| {
        c.content_preview
            .as_deref()
            .unwrap_or("")
            .contains("phoenix")
    }));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
chunk detail is read through the bound driver, not the in-process engine"]
async fn read_chunk_row_returns_preview_and_metadata() {
    let (_tmp, cfg) = test_config();
    seed_chat_chunk(
        &cfg,
        "slack:#eng",
        "phoenix migration scheduled friday with context and source refs",
    )
    .await;
    let chunk = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks
        .into_iter()
        .next()
        .expect("seeded chunk");

    let row = read_chunk_row(&chunk.id).await.unwrap().expect("chunk row");
    assert_eq!(row.id, chunk.id);
    assert_eq!(row.source_kind, "chat");
    assert_eq!(row.source_id, "slack:#eng");
    assert_eq!(row.source_ref.as_deref(), Some("slack://x"));
    assert_eq!(row.owner, "alice");
    assert_eq!(row.lifecycle_status, "pending_extraction");
    assert!(row.content_path.is_some());
    assert!(row
        .content_preview
        .as_deref()
        .unwrap_or("")
        .contains("phoenix migration scheduled friday"));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
chunk detail is read through the bound driver, not the in-process engine"]
async fn read_chunk_row_falls_back_to_sqlite_preview_when_file_missing() {
    let (_tmp, cfg) = test_config();
    let body = "sqlite preview survives missing file";
    seed_chat_chunk(&cfg, "slack:#eng", body).await;
    let chunk = list_chunks_rpc(&cfg, ChunkFilter::default())
        .await
        .unwrap()
        .value
        .chunks
        .into_iter()
        .next()
        .expect("seeded chunk");

    let rel_path = chunk.content_path.clone().expect("content path present");
    let abs_path = cfg.memory_tree_content_root().join(rel_path);
    std::fs::remove_file(&abs_path).expect("remove chunk file");

    let row = read_chunk_row(&chunk.id).await.unwrap().expect("chunk row");
    assert_eq!(row.content_path, chunk.content_path);
    assert!(row.content_preview.as_deref().unwrap_or("").contains(body));
}

/// The handler forwards the driver's flush outcome onto the wire unchanged.
///
/// The behaviour this test used to stage — an ingest producing a stale buffer,
/// the second flush deduplicating inside the window — is the *driver's* and
/// moved with the SQL to the conformance suite
/// (`flushing_twice_in_a_window_schedules_the_work_once`), where a real store
/// exists. What is the host's here is only the mapping: both fields pass
/// through, and the u64→u32 buffer count clamps rather than wraps.
#[tokio::test]
async fn flush_now_reports_the_drivers_outcome() {
    use crate::openhuman::memory::api::provider::types::FlushOutcome;

    let (_tmp, cfg) = test_config();
    crate::openhuman::memory::binding::install_for_test(
        &cfg.workspace_dir,
        &cfg.subsystems.memory,
        std::sync::Arc::new(
            crate::openhuman::memory::binding::FixedDiagnostics::new(
                Default::default(),
                Default::default(),
            )
            .flushing(FlushOutcome {
                enqueued: false,
                stale_buffers: u64::from(u32::MAX) + 7,
            }),
        ) as std::sync::Arc<dyn crate::openhuman::memory::api::provider::MemoryProvider>,
    );

    let resp = flush_now_rpc(&cfg).await.expect("flush_now").value;
    assert!(
        !resp.enqueued,
        "`enqueued: false` passes through — with a non-zero buffer count it \
         means \"already scheduled\", not \"nothing to do\""
    );
    assert_eq!(
        resp.stale_buffers,
        u32::MAX,
        "a count past the wire type's range clamps rather than wraps to a small lie"
    );
}
