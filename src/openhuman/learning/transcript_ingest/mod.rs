//! Transcript-to-memory ingestion pipeline.
//!
//! Reads completed session transcripts (`session_raw/*.jsonl`) and extracts
//! durable conversational memory plus higher-level reflections so that fresh
//! chats can recover continuity from prior conversations. See issue #1399.
//!
//! ## Outputs
//!
//! Two distinct memory streams, each persisted via [`crate::openhuman::memory::Memory`]:
//!
//! - **Conversational memory** (`conversation_memory` namespace) — durable
//!   facts (preferences, decisions, commitments, unresolved tasks) tagged with
//!   importance + provenance pointing back at the source transcript.
//! - **Conversational reflections** (`conversation_reflections` namespace) —
//!   higher-level patterns, recurring themes, or improvement signals.
//!
//! ## Pipeline
//!
//! ```text
//! SessionTranscript → extract → dedupe → persist → IngestionReport
//! ```
//!
//! Heuristic-only by design: the goal of the first pass is to make the
//! pipeline available to the rest of the system *without* a hard LLM
//! dependency, so it can run as a background task on session close, in tests,
//! and on machines without provider credentials. A subsequent iteration can
//! layer an LLM-driven extractor on the same trait surface.
//!
//! ## Provenance
//!
//! Every persisted entry carries enough metadata (`thread_id`, transcript
//! basename, source message indices, RFC-3339 timestamp) to trace the memory
//! back to the conversation it came from and to deduplicate repeats.

mod dedupe;
mod extract;
mod persist;
pub mod types;

pub use types::{
    CandidateKind, ConversationReflection, Importance, IngestionReport, MemoryCandidate,
    Provenance, CONVERSATION_MEMORY_NAMESPACE, CONVERSATION_REFLECTIONS_NAMESPACE,
};

use crate::openhuman::agent::harness::session::transcript::{self, SessionTranscript};
use crate::openhuman::memory::Memory;
use futures::stream::StreamExt;
use std::path::Path;

/// Max number of persist round-trips (markdown write + SQLite tx + embedding)
/// kept in flight at once during ingestion. Bounds provider load so a
/// transcript yielding dozens of candidates can't open dozens of concurrent
/// embedding requests.
const PERSIST_CONCURRENCY: usize = 8;

/// Ingest a single session transcript file: extract memory candidates,
/// dedupe against what's already stored, and persist new entries.
///
/// Background-first: callers should invoke this from a `tokio::spawn` so
/// chat latency is unaffected (see
/// `Agent::spawn_transcript_ingestion`). Failures are returned but the
/// caller should generally just log them — ingestion is best-effort and
/// retried on the next transcript write.
pub async fn ingest_transcript_path(
    memory: &dyn Memory,
    path: &Path,
) -> anyhow::Result<IngestionReport> {
    log::debug!("[transcript_ingest] starting ingest for {}", path.display());
    let parsed = transcript::read_transcript(path)?;
    ingest_session_transcript(memory, &parsed, path).await
}

/// Ingest an already-parsed [`SessionTranscript`].
///
/// Exposed separately from `ingest_transcript_path` so tests can drive the
/// pipeline without touching the filesystem.
pub async fn ingest_session_transcript(
    memory: &dyn Memory,
    transcript: &SessionTranscript,
    path: &Path,
) -> anyhow::Result<IngestionReport> {
    let basename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let path_display = path.display().to_string();
    let thread_id = transcript.meta.thread_id.clone();
    let now = chrono::Utc::now().to_rfc3339();

    let extracted = extract::extract_candidates(
        &transcript.messages,
        &extract::Provenance {
            thread_id: thread_id.clone(),
            transcript_path: path_display.clone(),
            transcript_basename: basename.clone(),
            extracted_at: now.clone(),
        },
    );

    let reflections = extract::extract_reflections(
        &transcript.messages,
        &extract::Provenance {
            thread_id: thread_id.clone(),
            transcript_path: path_display.clone(),
            transcript_basename: basename.clone(),
            extracted_at: now,
        },
    );

    let extracted_total = extracted.len();
    let reflection_total = reflections.len();

    let (kept, deduped) = dedupe::filter_new(memory, extracted).await?;
    let (kept_reflections, deduped_reflections) =
        dedupe::filter_new_reflections(memory, reflections).await?;

    // Persist kept candidates + reflections with BOUNDED concurrency instead
    // of one sequential await each. A single transcript can yield dozens of
    // items, and each persist is a markdown write + SQLite tx + an embedding
    // round-trip — overlapping them (capped at PERSIST_CONCURRENCY) lets their
    // network/disk waits run in parallel, finishing the background ingest job
    // sooner, without opening an unbounded number of concurrent embed requests.
    // Order is irrelevant here — only the per-item Ok/Err accounting matters.
    // Each future logs its own error and yields 1 on success / 0 on failure,
    // so no borrowed candidate reference crosses the stream-combinator
    // boundary (which otherwise trips a higher-ranked-lifetime error). The
    // fold just sums the per-item outcomes.
    // Collect the per-item futures into a `Vec` *before* handing them to
    // `buffer_unordered`. Mapping lazily on the stream (`stream::iter(it.map(..))`)
    // would store the closure in the polled state and require it to hold for
    // any lifetime (HRTB), which fails to compile once the whole ingest future
    // is spawned (`Send + 'static`). Collecting runs each closure up front, so
    // the stream only carries already-built futures with concrete lifetimes.
    let candidate_futs: Vec<_> = kept
        .iter()
        .map(|candidate| async move {
            match persist::store_candidate(memory, candidate).await {
                Ok(()) => 1usize,
                Err(err) => {
                    log::warn!(
                        "[transcript_ingest] failed to persist candidate kind={:?} importance={:?}: {err}",
                        candidate.kind,
                        candidate.importance
                    );
                    0usize
                }
            }
        })
        .collect();
    let stored = futures::stream::iter(candidate_futs)
        .buffer_unordered(PERSIST_CONCURRENCY)
        .fold(0usize, |acc, n| async move { acc + n })
        .await;

    let reflection_futs: Vec<_> = kept_reflections
        .iter()
        .map(|reflection| async move {
            match persist::store_reflection(memory, reflection).await {
                Ok(()) => 1usize,
                Err(err) => {
                    log::warn!("[transcript_ingest] failed to persist reflection: {err}");
                    0usize
                }
            }
        })
        .collect();
    let stored_reflections = futures::stream::iter(reflection_futs)
        .buffer_unordered(PERSIST_CONCURRENCY)
        .fold(0usize, |acc, n| async move { acc + n })
        .await;

    log::info!(
        "[transcript_ingest] ingested {}: extracted={} stored={} deduped={} reflections={}/{} (deduped={}) thread={}",
        path.display(),
        extracted_total,
        stored,
        deduped,
        stored_reflections,
        reflection_total,
        deduped_reflections,
        thread_id.as_deref().unwrap_or("-"),
    );

    Ok(IngestionReport {
        processed_messages: transcript.messages.len(),
        extracted: extracted_total,
        stored,
        deduped,
        reflections_extracted: reflection_total,
        reflections_stored: stored_reflections,
        candidates: kept,
        reflections: kept_reflections,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
