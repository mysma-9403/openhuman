//! Per-source sync status — chunks ingested, freshness, in-flight progress.
//!
//! Queries `mem_tree_chunks` filtered by source-id prefix:
//! - Reader-backed kinds (folder/github/rss/web/twitter) tag chunks
//!   with `mem_src:{source.id}:%`, so we count those directly.
//! - Composio sources tag chunks with the toolkit-specific id
//!   (e.g. `gmail:user@example.com:msg_xxx`), so we match by toolkit
//!   prefix instead.
//!
//! "Pending" means *not yet resolved for the active embedding signature*. A
//! chunk is resolved when it has a vector in the `mem_tree_chunk_embeddings`
//! sidecar under [`tree_active_signature`], has a re-embed tombstone for that
//! signature, was dropped by the admission gate, or still carries a legacy
//! pre-sidecar `embedding` blob. This mirrors the resolution rule the
//! provider-level sibling (`tinycortex::memory::sync::list_sync_statuses`) and
//! `has_uncovered_reembed_work` already use, so a settled store reports zero
//! instead of reporting every ingested chunk as pending forever.

use std::fmt::Write as _;

use rusqlite::Connection;
use serde::Serialize;

use crate::openhuman::config::Config;
use crate::openhuman::memory::sources::types::{MemorySourceEntry, SourceKind};
use crate::openhuman::memory::store::chunks::store::{
    tree_active_signature, with_connection, CHUNK_STATUS_DROPPED,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessLabel {
    Active,
    Recent,
    Idle,
}

impl FreshnessLabel {
    pub fn from_age_ms(last_ms: Option<i64>, now_ms: i64) -> Self {
        match last_ms {
            None => Self::Idle,
            Some(ts) => {
                let age = now_ms.saturating_sub(ts);
                if age <= 30_000 {
                    Self::Active
                } else if age <= 5 * 60_000 {
                    Self::Recent
                } else {
                    Self::Idle
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub source_id: String,
    pub chunks_synced: u64,
    pub chunks_pending: u64,
    pub last_chunk_at_ms: Option<i64>,
    pub freshness: FreshnessLabel,
}

/// Compute status for one source.
pub async fn source_status(
    config: &Config,
    source: &MemorySourceEntry,
) -> Result<SourceStatus, String> {
    let cfg = config.clone();
    let source_clone = source.clone();

    tokio::task::spawn_blocking(move || {
        // Embeddings are scoped per (chunk, model signature); a vector stored
        // under a superseded signature is unreachable in the active vector
        // space, so it must still read as pending.
        let signature = tree_active_signature(&cfg);

        with_connection(&cfg, |conn| {
            let prefix = source_id_prefix(&source_clone);

            // Surface real query errors so status telemetry doesn't lie about
            // a healthy zero-row state when the DB is actually broken.
            //
            // The trailing `c.embedding IS NOT NULL` term is a compatibility
            // clause for vaults that predate the embedding sidecar:
            // `migrate_legacy_embeddings_to_sidecar` copies those blobs into
            // `mem_tree_chunk_embeddings` but deliberately preserves the legacy
            // column, so it remains a valid "this chunk was embedded" signal.
            // It is inert for anything ingested after the sidecar landed.
            let (synced, pending, last_ts): (i64, i64, Option<i64>) = conn.query_row(
                "SELECT \
                       COUNT(*), \
                       SUM(CASE WHEN EXISTS ( \
                                    SELECT 1 FROM mem_tree_chunk_embeddings e \
                                     WHERE e.chunk_id = c.id \
                                       AND e.model_signature = ?2) \
                                 OR EXISTS ( \
                                    SELECT 1 FROM mem_tree_chunk_reembed_skipped s \
                                     WHERE s.chunk_id = c.id \
                                       AND s.model_signature = ?2) \
                                 OR c.lifecycle_status = ?3 \
                                 OR c.embedding IS NOT NULL \
                                THEN 0 ELSE 1 END), \
                       MAX(c.timestamp_ms) \
                     FROM mem_tree_chunks c \
                     WHERE c.source_id LIKE ?1",
                rusqlite::params![prefix, signature, CHUNK_STATUS_DROPPED],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get(2)?,
                    ))
                },
            )?;

            let now_ms = chrono::Utc::now().timestamp_millis();
            Ok(SourceStatus {
                source_id: source_clone.id.clone(),
                chunks_synced: synced.max(0) as u64,
                chunks_pending: pending.max(0) as u64,
                last_chunk_at_ms: last_ts,
                freshness: FreshnessLabel::from_age_ms(last_ts, now_ms),
            })
        })
        .map_err(|e| format!("source_status: {e}"))
    })
    .await
    .map_err(|e| format!("source_status join: {e}"))?
}

/// Number of sources folded into one `mem_tree_chunks` scan. Each source
/// contributes three aggregate columns and one bound prefix; the shared
/// signature + dropped-status binds add two more. 128 keeps both well under
/// SQLite's default 2000-column / 999-parameter statement limits
/// (128 × 3 = 384 columns, 130 binds).
const STATUS_BATCH_SOURCES: usize = 128;

/// Compute status for all configured sources.
///
/// Fast path folds every source's counts into `ceil(N / STATUS_BATCH_SOURCES)`
/// `mem_tree_chunks` scans instead of one scan per source. The Memory Sources
/// panel polls this every 5s (`MemorySourcesRegistry.tsx`), and every read
/// shares the one process-wide chunk-DB connection mutex — so issuing fewer
/// scans, not concurrency, is the only lever. If the batched query fails for any
/// reason the per-source path still runs, so a query regression degrades
/// latency, never correctness.
pub async fn status_list(config: &Config) -> Result<Vec<SourceStatus>, String> {
    let sources = crate::openhuman::memory::sources::registry::list_sources().await?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    tracing::debug!(
        source_count = sources.len(),
        "[memory_sources:status] status_list: entry"
    );
    match batch_status(config, &sources).await {
        Ok(statuses) => {
            tracing::debug!(
                source_count = statuses.len(),
                "[memory_sources:status] status_list: batched fast path ok"
            );
            Ok(statuses)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                source_count = sources.len(),
                "[memory_sources:status] batched status query failed; falling back to per-source path"
            );
            status_list_per_source(config, &sources).await
        }
    }
}

/// Per-source fallback: one `source_status` round-trip per source. A single
/// source's failure degrades only its own row (Idle / zero), never the list.
async fn status_list_per_source(
    config: &Config,
    sources: &[MemorySourceEntry],
) -> Result<Vec<SourceStatus>, String> {
    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        match source_status(config, source).await {
            Ok(s) => out.push(s),
            Err(e) => {
                tracing::warn!(
                    source_id = %source.id,
                    error = %e,
                    "[memory_sources:status] query failed"
                );
                out.push(SourceStatus {
                    source_id: source.id.clone(),
                    chunks_synced: 0,
                    chunks_pending: 0,
                    last_chunk_at_ms: None,
                    freshness: FreshnessLabel::Idle,
                });
            }
        }
    }
    Ok(out)
}

/// Aggregate every source's chunk counts in single-scan batches, preserving the
/// exact per-source semantics of [`source_status`] (see [`aggregate_prefixes`]).
async fn batch_status(
    config: &Config,
    sources: &[MemorySourceEntry],
) -> Result<Vec<SourceStatus>, String> {
    let cfg = config.clone();
    let prefixes: Vec<String> = sources.iter().map(source_id_prefix).collect();
    let source_ids: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
    tracing::debug!(
        source_count = prefixes.len(),
        batch_count = prefixes.len().div_ceil(STATUS_BATCH_SOURCES),
        batch_size = STATUS_BATCH_SOURCES,
        "[memory_sources:status] batch_status: aggregating over mem_tree_chunks in single-scan batches"
    );
    let rows: Vec<(i64, i64, Option<i64>)> = tokio::task::spawn_blocking(move || {
        // Signature is scoped per active embedding model, not per source —
        // resolve it once and reuse across every batch (matches `source_status`).
        let signature = tree_active_signature(&cfg);
        with_connection(&cfg, |conn| {
            let mut out = Vec::with_capacity(prefixes.len());
            for batch in prefixes.chunks(STATUS_BATCH_SOURCES) {
                out.extend(aggregate_prefixes(conn, batch, &signature)?);
            }
            Ok(out)
        })
        .map_err(|e| format!("batch_status: {e}"))
    })
    .await
    .map_err(|e| format!("batch_status join: {e}"))??;

    let now_ms = chrono::Utc::now().timestamp_millis();
    Ok(source_ids
        .into_iter()
        .zip(rows)
        .map(|(source_id, (synced, pending, last_ms))| SourceStatus {
            source_id,
            chunks_synced: synced.max(0) as u64,
            chunks_pending: pending.max(0) as u64,
            last_chunk_at_ms: last_ms,
            freshness: FreshnessLabel::from_age_ms(last_ms, now_ms),
        })
        .collect())
}

/// One `mem_tree_chunks` scan yielding `(synced, pending, last_ts)` for each
/// prefix, in order.
///
/// Each source contributes three aggregate columns gated by `source_id LIKE ?n`.
/// The pending column embeds [`source_status`]'s exact resolved-predicate — a
/// sidecar vector under the active signature, a re-embed tombstone, a dropped
/// lifecycle, or a legacy blob — *inside* the gate's `THEN` branch. Two reasons
/// for the nested `CASE` rather than a flat `LIKE ?n AND NOT(<predicate>)`:
///
/// 1. **Correctness.** `source_status` counts a row as pending whenever the
///    predicate is false *or NULL* (`CASE WHEN <pred> THEN 0 ELSE 1`). A bare
///    `NOT(<pred>)` maps NULL → NULL → not-counted, silently under-counting the
///    NULL-`lifecycle_status` rows. Nesting the identical `CASE` reproduces the
///    NULL-→-pending behaviour exactly.
/// 2. **Cost.** SQL evaluates only the taken `CASE` branch, so the `EXISTS`
///    sub-selects run only for a row's own source — never once per source per
///    row — without relying on `AND` short-circuit.
fn aggregate_prefixes(
    conn: &Connection,
    prefixes: &[String],
    signature: &str,
) -> anyhow::Result<Vec<(i64, i64, Option<i64>)>> {
    let n = prefixes.len();
    let sig_p = n + 1;
    let dropped_p = n + 2;
    let mut columns = String::new();
    for i in 0..n {
        let p = i + 1;
        if i > 0 {
            columns.push_str(", ");
        }
        // Writing to a `String` via `fmt::Write` is infallible; assert that here
        // rather than threading a `?` that can never actually fire.
        write!(
            columns,
            "SUM(CASE WHEN c.source_id LIKE ?{p} THEN 1 ELSE 0 END), \
             SUM(CASE WHEN c.source_id LIKE ?{p} THEN \
                    (CASE WHEN EXISTS ( \
                                 SELECT 1 FROM mem_tree_chunk_embeddings e \
                                  WHERE e.chunk_id = c.id AND e.model_signature = ?{sig_p}) \
                              OR EXISTS ( \
                                 SELECT 1 FROM mem_tree_chunk_reembed_skipped s \
                                  WHERE s.chunk_id = c.id AND s.model_signature = ?{sig_p}) \
                              OR c.lifecycle_status = ?{dropped_p} \
                              OR c.embedding IS NOT NULL \
                             THEN 0 ELSE 1 END) \
                  ELSE 0 END), \
             MAX(CASE WHEN c.source_id LIKE ?{p} THEN c.timestamp_ms END)"
        )
        .expect("writing to a String never fails");
    }
    let where_clause = (1..=n)
        .map(|p| format!("c.source_id LIKE ?{p}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!("SELECT {columns} FROM mem_tree_chunks c WHERE {where_clause}");
    tracing::trace!(
        prefix_count = n,
        "[memory_sources:status] aggregate_prefixes: scanning mem_tree_chunks for one batch"
    );

    // Bind order matches the numbered params: prefixes ?1..?N, then the shared
    // signature ?N+1 and dropped-status ?N+2.
    let mut binds: Vec<String> = prefixes.to_vec();
    binds.push(signature.to_string());
    binds.push(CHUNK_STATUS_DROPPED.to_string());

    let triples = conn.query_row(&sql, rusqlite::params_from_iter(binds.iter()), |r| {
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let base = i * 3;
            let synced = r.get::<_, Option<i64>>(base)?.unwrap_or(0);
            let pending = r.get::<_, Option<i64>>(base + 1)?.unwrap_or(0);
            let last_ms = r.get::<_, Option<i64>>(base + 2)?;
            v.push((synced, pending, last_ms));
        }
        Ok(v)
    })?;
    Ok(triples)
}

/// Build the `source_id LIKE` prefix that matches chunks belonging to a source.
fn source_id_prefix(source: &MemorySourceEntry) -> String {
    match source.kind {
        SourceKind::Composio => {
            // Composio providers write chunks with source_id = `{toolkit}:%`
            // (e.g. `gmail:user@example.com:msg_xxx`). Match by toolkit only.
            source
                .toolkit
                .as_deref()
                .map(|t| format!("{t}:%"))
                .unwrap_or_else(|| "__no_toolkit__:%".to_string())
        }
        _ => format!("mem_src:{}:%", source.id),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    use super::*;
    use crate::openhuman::memory::store::chunks::store::{
        mark_chunk_reembed_skipped, set_chunk_embedding_for_signature, set_chunk_lifecycle_status,
        upsert_chunks,
    };
    use crate::openhuman::memory::store::chunks::types::{
        chunk_id, Chunk, Metadata, SourceKind as ChunkSourceKind,
    };

    fn test_config() -> (TempDir, Config) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        (tmp, cfg)
    }

    fn source_entry(id: &str) -> MemorySourceEntry {
        MemorySourceEntry {
            id: id.into(),
            kind: SourceKind::Folder,
            label: "x".into(),
            enabled: true,
            toolkit: None,
            connection_id: None,
            path: Some("/tmp".into()),
            glob: None,
            url: None,
            branch: None,
            paths: Vec::new(),
            query: None,
            since_days: None,
            max_items: None,
            max_commits: None,
            max_issues: None,
            max_prs: None,
            selector: None,
            max_tokens_per_sync: None,
            max_cost_per_sync_usd: None,
            sync_depth_days: None,
        }
    }

    fn chunk(source_id: &str, seq: u32, timestamp_ms: i64) -> Chunk {
        let ts = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
        let content = format!("status chunk {source_id} #{seq}");
        Chunk {
            id: chunk_id(ChunkSourceKind::Document, source_id, seq, &content),
            content,
            metadata: Metadata::point_in_time(ChunkSourceKind::Document, source_id, "test", ts),
            token_count: 1,
            seq_in_source: seq,
            created_at: ts,
            partial_message: false,
        }
    }

    fn seed(cfg: &Config, source: &str, count: u32) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        for seq in 0..count {
            let source_id = format!("mem_src:{source}:item-{seq}");
            chunks.push(chunk(&source_id, seq, 1_700_000_000_000 + i64::from(seq)));
        }
        upsert_chunks(cfg, &chunks).unwrap();
        chunks
    }

    async fn status_of(cfg: &Config, id: &str) -> SourceStatus {
        source_status(cfg, &source_entry(id)).await.unwrap()
    }

    #[test]
    fn freshness_thresholds() {
        let now = 1_000_000_000_000;
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 1_000), now),
            FreshnessLabel::Active
        );
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 60_000), now),
            FreshnessLabel::Recent
        );
        assert_eq!(
            FreshnessLabel::from_age_ms(Some(now - 600_000), now),
            FreshnessLabel::Idle
        );
        assert_eq!(FreshnessLabel::from_age_ms(None, now), FreshnessLabel::Idle);
    }

    #[test]
    fn source_id_prefix_dispatch() {
        let mut entry = source_entry("src_abc");
        assert_eq!(source_id_prefix(&entry), "mem_src:src_abc:%");

        entry.kind = SourceKind::Composio;
        entry.toolkit = Some("gmail".into());
        assert_eq!(source_id_prefix(&entry), "gmail:%");
    }

    /// The reported bug (#5329): the old query counted pending as
    /// `mem_tree_chunks.embedding IS NULL`, and no production writer populates
    /// that column, so `pending` always equalled `synced`. Reading the sidecar
    /// lets a fully-embedded source settle at zero.
    #[tokio::test]
    async fn pending_reaches_zero_once_every_chunk_has_an_active_embedding() {
        let (_tmp, cfg) = test_config();
        let chunks = seed(&cfg, "src_done", 2);
        let active = tree_active_signature(&cfg);
        for c in &chunks {
            set_chunk_embedding_for_signature(&cfg, &c.id, &active, &[0.5]).unwrap();
        }

        let status = status_of(&cfg, "src_done").await;
        assert_eq!(status.chunks_synced, 2);
        assert_eq!(
            status.chunks_pending, 0,
            "pre-fix this reported pending == synced"
        );
    }

    /// A vector stored under a superseded model signature does not make the
    /// chunk reachable in the active vector space, so it must stay pending.
    #[tokio::test]
    async fn pending_ignores_embeddings_from_a_superseded_signature() {
        let (_tmp, cfg) = test_config();
        let chunks = seed(&cfg, "src_mixed", 3);
        let active = tree_active_signature(&cfg);
        set_chunk_embedding_for_signature(&cfg, &chunks[0].id, &active, &[0.1, 0.2]).unwrap();
        set_chunk_embedding_for_signature(&cfg, &chunks[1].id, "stale/model@7", &[0.3]).unwrap();

        let status = status_of(&cfg, "src_mixed").await;
        assert_eq!(status.chunks_synced, 3);
        assert_eq!(
            status.chunks_pending, 2,
            "only the active-signature vector resolves a chunk"
        );
    }

    /// Chunks that will never be embedded — a re-embed tombstone for the active
    /// signature, or a chunk the admission gate dropped — must not be counted,
    /// otherwise the counter can never drain. Matches the resolution rule in
    /// `tinycortex::memory::sync::list_sync_statuses`.
    #[tokio::test]
    async fn pending_excludes_tombstoned_and_dropped_chunks() {
        let (_tmp, cfg) = test_config();
        let chunks = seed(&cfg, "src_terminal", 3);
        let active = tree_active_signature(&cfg);
        mark_chunk_reembed_skipped(&cfg, &chunks[0].id, &active, "too large").unwrap();
        set_chunk_lifecycle_status(&cfg, &chunks[1].id, CHUNK_STATUS_DROPPED).unwrap();

        let status = status_of(&cfg, "src_terminal").await;
        assert_eq!(status.chunks_synced, 3);
        assert_eq!(status.chunks_pending, 1, "only the untouched chunk pends");
    }

    /// Vaults created before the embedding sidecar keep their vector in the
    /// legacy `mem_tree_chunks.embedding` column, which
    /// `migrate_legacy_embeddings_to_sidecar` preserves. Those chunks stay
    /// resolved so an old vault does not regress to "everything pending".
    #[tokio::test]
    async fn pending_treats_a_legacy_pre_sidecar_embedding_as_resolved() {
        let (_tmp, cfg) = test_config();
        let chunks = seed(&cfg, "src_legacy", 2);
        with_connection(&cfg, |conn| {
            conn.execute(
                "UPDATE mem_tree_chunks SET embedding = X'00010203' WHERE id = ?1",
                [&chunks[0].id],
            )?;
            Ok(())
        })
        .unwrap();

        let status = status_of(&cfg, "src_legacy").await;
        assert_eq!(status.chunks_synced, 2);
        assert_eq!(status.chunks_pending, 1);
    }

    /// A source with no chunks at all must report zeroes rather than erroring
    /// on the `SUM`/`MAX` NULLs an empty scan produces.
    #[tokio::test]
    async fn empty_source_reports_zeroed_status() {
        let (_tmp, cfg) = test_config();
        let status = status_of(&cfg, "src_empty").await;
        assert_eq!(status.chunks_synced, 0);
        assert_eq!(status.chunks_pending, 0);
        assert_eq!(status.last_chunk_at_ms, None);
        assert_eq!(status.freshness, FreshnessLabel::Idle);
    }

    /// The batched fast path must be byte-for-byte equivalent to the per-source
    /// `source_status` across every resolution state the pending predicate
    /// distinguishes: active-signature embed, superseded-signature embed,
    /// re-embed tombstone, dropped lifecycle, legacy blob, and genuinely pending.
    #[tokio::test]
    async fn batch_status_matches_per_source_counts() {
        let (_tmp, cfg) = test_config();
        let active = tree_active_signature(&cfg);

        // src_done: every chunk embedded under the active signature → 2/0.
        let done = seed(&cfg, "src_done", 2);
        for c in &done {
            set_chunk_embedding_for_signature(&cfg, &c.id, &active, &[0.5]).unwrap();
        }
        // src_mixed: active, stale-signature, untouched → 3/2.
        let mixed = seed(&cfg, "src_mixed", 3);
        set_chunk_embedding_for_signature(&cfg, &mixed[0].id, &active, &[0.1]).unwrap();
        set_chunk_embedding_for_signature(&cfg, &mixed[1].id, "stale/model@7", &[0.3]).unwrap();
        // src_terminal: re-embed tombstone, dropped, untouched → 3/1.
        let terminal = seed(&cfg, "src_terminal", 3);
        mark_chunk_reembed_skipped(&cfg, &terminal[0].id, &active, "too large").unwrap();
        set_chunk_lifecycle_status(&cfg, &terminal[1].id, CHUNK_STATUS_DROPPED).unwrap();
        // src_legacy: one legacy pre-sidecar embedding blob → 2/1.
        let legacy = seed(&cfg, "src_legacy", 2);
        with_connection(&cfg, |conn| {
            conn.execute(
                "UPDATE mem_tree_chunks SET embedding = X'00010203' WHERE id = ?1",
                [&legacy[0].id],
            )?;
            Ok(())
        })
        .unwrap();
        // src_plain: nothing resolved → 2/2. src_empty: no chunks → 0/0.
        seed(&cfg, "src_plain", 2);

        let sources: Vec<MemorySourceEntry> = [
            "src_done",
            "src_mixed",
            "src_terminal",
            "src_legacy",
            "src_plain",
            "src_empty",
        ]
        .iter()
        .map(|id| source_entry(id))
        .collect();

        let batched = batch_status(&cfg, &sources).await.unwrap();
        assert_eq!(batched.len(), sources.len());
        for (i, source) in sources.iter().enumerate() {
            let expected = source_status(&cfg, source).await.unwrap();
            let got = &batched[i];
            assert_eq!(got.source_id, expected.source_id);
            assert_eq!(
                got.chunks_synced, expected.chunks_synced,
                "synced mismatch for {}",
                source.id
            );
            assert_eq!(
                got.chunks_pending, expected.chunks_pending,
                "pending mismatch for {}",
                source.id
            );
            assert_eq!(
                got.last_chunk_at_ms, expected.last_chunk_at_ms,
                "last_ms mismatch for {}",
                source.id
            );
            assert_eq!(
                got.freshness, expected.freshness,
                "freshness mismatch for {}",
                source.id
            );
        }

        // Absolute counts too, so a bug that corrupts BOTH paths identically
        // (and would pass the equivalence loop above) still fails here.
        let counts: Vec<(u64, u64)> = batched
            .iter()
            .map(|s| (s.chunks_synced, s.chunks_pending))
            .collect();
        assert_eq!(counts, vec![(2, 0), (3, 2), (3, 1), (2, 1), (2, 2), (0, 0)]);
        assert_eq!(batched[5].last_chunk_at_ms, None);
    }

    /// Exceed one query batch so the chunked scan and its index→source mapping
    /// are exercised across a batch boundary; a mis-aligned `zip` would surface
    /// as a per-index count mismatch.
    #[tokio::test]
    async fn batch_status_spans_multiple_query_batches() {
        let (_tmp, cfg) = test_config();
        let n = STATUS_BATCH_SOURCES + 5;
        let mut sources = Vec::with_capacity(n);
        for i in 0..n {
            let id = format!("src_{i:04}");
            // Vary the count per source so a boundary misalignment is visible.
            seed(&cfg, &id, (i % 3) as u32 + 1);
            sources.push(source_entry(&id));
        }

        let batched = batch_status(&cfg, &sources).await.unwrap();
        assert_eq!(batched.len(), n);
        for (i, st) in batched.iter().enumerate() {
            let expected = (i % 3) as u64 + 1;
            assert_eq!(st.source_id, format!("src_{i:04}"));
            assert_eq!(st.chunks_synced, expected, "synced mismatch at index {i}");
            // Nothing was embedded, so every chunk still pends.
            assert_eq!(st.chunks_pending, expected, "pending mismatch at index {i}");
        }
    }

    /// No chunks at all: the `SUM`/`MAX` over a zero-row scan yield SQL NULLs,
    /// which must decode to 0 / None rather than erroring the whole panel.
    #[tokio::test]
    async fn batch_status_empty_db_is_zeroed_not_errored() {
        let (_tmp, cfg) = test_config();
        let sources: Vec<MemorySourceEntry> =
            ["a", "b", "c"].iter().map(|id| source_entry(id)).collect();
        let batched = batch_status(&cfg, &sources).await.unwrap();
        assert_eq!(batched.len(), 3);
        for st in &batched {
            assert_eq!(st.chunks_synced, 0);
            assert_eq!(st.chunks_pending, 0);
            assert_eq!(st.last_chunk_at_ms, None);
            assert_eq!(st.freshness, FreshnessLabel::Idle);
        }
    }
}
