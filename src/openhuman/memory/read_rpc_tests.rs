use super::*;
use crate::openhuman::inference::embeddings::NoopEmbedding;
use chrono::{TimeZone, Utc};
use rusqlite::params;
use std::sync::Arc;
use tempfile::TempDir;
use tinycortex::memory::ingest::canonicalize::chat::{ChatBatch, ChatMessage};
use tinycortex::memory::sync::state::STATE_NAMESPACE as KV_NAMESPACE;
use tinymemory_core::ingest_pipeline::ingest_chat;
use tinymemory_core::queue::drain_until_idle;
use tinymemory_core::store::content::raw::{write_raw_items, RawItem, RawKind};
use tinymemory_core::store::namespace_store::UnifiedMemory;

fn test_config() -> (TempDir, Config) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::default();
    cfg.workspace_dir = tmp.path().to_path_buf();
    // Point config_path inside the tempdir so any persistence during
    // tests stays inside disposable workspace state.
    cfg.config_path = tmp.path().join("config.toml");
    cfg.memory_tree.embedding_endpoint = None;
    cfg.memory_tree.embedding_model = None;
    cfg.memory_tree.embedding_strict = false;
    // Default llm is Cloud — but the cloud provider needs a bearer
    // token to actually fire. Tests that exercise the LLM path
    // override either the backend or the extractor. The read RPCs
    // below don't touch the LLM, so this default is fine.
    (tmp, cfg)
}

async fn seed_chat_chunk(cfg: &Config, source: &str, body: &str) {
    let batch = ChatBatch {
        platform: "slack".into(),
        channel_label: source.into(),
        messages: vec![ChatMessage {
            author: "alice".into(),
            timestamp: Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            text: body.into(),
            source_ref: Some("slack://x".into()),
        }],
    };
    ingest_chat(cfg, source, "alice", vec![], batch)
        .await
        .unwrap();
}

async fn seed_slack_chunk_with_raw_archive(cfg: &Config) -> String {
    let timestamp = Utc.timestamp_opt(1_700_000_000, 0).single().unwrap();
    write_raw_items(
        &cfg.memory_tree_content_root(),
        "slack:conn-slack-1",
        &[RawItem {
            uid: "1700000000.000100",
            created_at_ms: timestamp.timestamp_millis(),
            markdown: "**Channel:** #engineering\n**Author:** alice\n\nPhoenix migration launch window is Friday at 22:00 UTC.",
            kind: RawKind::Chat,
        }],
    )
    .expect("seed raw Slack artifact");
    let batch = ChatBatch {
        platform: "slack".into(),
        channel_label: "#engineering".into(),
        messages: vec![ChatMessage {
            author: "alice".into(),
            timestamp,
            text: "Phoenix migration launch window is Friday at 22:00 UTC.".into(),
            source_ref: Some("slack://archives/C123/1700000000.000100".into()),
        }],
    };
    ingest_chat(
        cfg,
        "slack:conn-slack-1",
        "alice",
        vec!["slack".into(), "ingested".into()],
        batch,
    )
    .await
    .expect("seed slack ingest");
    drain_until_idle(cfg).await.expect("drain slack ingest");

    list_chunks_rpc(cfg, ChunkFilter::default())
        .await
        .expect("list chunks")
        .value
        .chunks
        .into_iter()
        .find(|chunk| chunk.source_id == "slack:conn-slack-1")
        .expect("seeded slack chunk")
        .id
}

fn update_chunk_timestamp(cfg: &Config, chunk_id: &str, timestamp_ms: i64) {
    with_connection(cfg, |conn| {
        conn.execute(
            "UPDATE mem_tree_chunks
                SET timestamp_ms = ?1,
                    time_range_start_ms = ?1,
                    time_range_end_ms = ?1
              WHERE id = ?2",
            params![timestamp_ms, chunk_id],
        )?;
        Ok(())
    })
    .unwrap();
}

fn insert_raw_chunk(
    cfg: &Config,
    id: &str,
    source_kind: &str,
    source_id: &str,
    timestamp_ms: i64,
    tags_json: &str,
    content: &str,
    token_count: i64,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT INTO mem_tree_chunks (
                id, source_kind, source_id, source_ref, owner, timestamp_ms,
                time_range_start_ms, time_range_end_ms, tags_json, content,
                token_count, seq_in_source, created_at_ms, lifecycle_status, content_path
             ) VALUES (?1, ?2, ?3, NULL, 'tester', ?4, ?4, ?4, ?5, ?6, ?7, 0, ?4, 'seeded', NULL)",
            params![
                id,
                source_kind,
                source_id,
                timestamp_ms,
                tags_json,
                content,
                token_count
            ],
        )?;
        Ok(())
    })
    .unwrap();
}

// ── tree-mode graph export (summaries + leaf chunks) ────────────────────

/// Insert a tree row and one summary node under it.
fn insert_tree_summary(cfg: &Config, tree_id: &str, scope: &str, summary_id: &str, level: i64) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO mem_tree_trees (id, kind, scope, created_at_ms)
             VALUES (?1, 'source', ?2, 0)",
            params![tree_id, scope],
        )?;
        conn.execute(
            "INSERT INTO mem_tree_summaries (
                id, tree_id, tree_kind, level, child_ids_json, content, token_count,
                entities_json, topics_json, time_range_start_ms, time_range_end_ms,
                score, sealed_at_ms, deleted
             ) VALUES (?1, ?2, 'source', ?3, '[]', 'summary body', 1, '[]', '[]', 0, 0, 0.0, 0, 0)",
            params![summary_id, tree_id, level],
        )?;
        Ok(())
    })
    .unwrap();
}

/// Insert a leaf chunk, optionally linked to a parent summary.
fn insert_chunk_with_parent(
    cfg: &Config,
    id: &str,
    parent_summary_id: Option<&str>,
    timestamp_ms: i64,
    content: &str,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT INTO mem_tree_chunks (
                id, source_kind, source_id, source_ref, owner, timestamp_ms,
                time_range_start_ms, time_range_end_ms, tags_json, content,
                token_count, seq_in_source, created_at_ms, lifecycle_status,
                content_path, parent_summary_id
             ) VALUES (?1, 'chat', 'slack:#eng', NULL, 'tester', ?2, ?2, ?2, '[]', ?3, 1, 0, ?2, 'seeded', NULL, ?4)",
            params![id, timestamp_ms, content, parent_summary_id],
        )?;
        Ok(())
    })
    .unwrap();
}

/// Insert one `mem_tree_entity_index` row.
///
/// Seeded directly because the `person` kind only ever comes from the LLM
/// extractor, which these tests deliberately do not run — the mechanical
/// extractor emits `email`/`url`/`handle`/`hashtag` and nothing else.
fn insert_entity_row(
    cfg: &Config,
    entity_id: &str,
    node_id: &str,
    entity_kind: &str,
    surface: &str,
    timestamp_ms: i64,
) {
    with_connection(cfg, |conn| {
        conn.execute(
            "INSERT OR REPLACE INTO mem_tree_entity_index (
                entity_id, node_id, node_kind, entity_kind, surface,
                score, timestamp_ms, tree_id, is_user
             ) VALUES (?1, ?2, 'leaf', ?3, ?4, 1.0, ?5, NULL, 0)",
            params![entity_id, node_id, entity_kind, surface, timestamp_ms],
        )?;
        Ok(())
    })
    .unwrap();
}

#[path = "read_rpc_tests_part_01_tests.rs"]
mod part_01_tests;
#[path = "read_rpc_tests_part_02_tests.rs"]
mod part_02_tests;
