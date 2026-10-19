//! Host adapter over the harness `CompletionRouter` for finished detached
//! background sub-agents (`spawn_async_subagent`).
//!
//! The queue itself is the harness's: `tinyagents_tasks::CompletionRouter`
//! deduplicates by task id, holds tombstones (collected inline, stopped,
//! cancelled parent), counts delivery attempts, hands back the records that
//! gave up, and persists every transition to a per-workspace
//! `DriverCompletionStore` (documents in the storage backend), so a completion that was never delivered is
//! delivered after a restart. This module is the thin host seam over it:
//!
//! * one router per workspace, opened lazily beside the task ledger
//!   (the `orchestration_completions` collection of the storage backend);
//! * the router's parent key is the **chat thread id** — delivery is
//!   thread-addressed, and a thread-scoped cancel (Stop, delete, purge) is then
//!   the router's own `cancel_parent` / `resume_parent`;
//! * the product-specific lifecycle helpers (`record_failure`,
//!   `record_awaiting_input`, `mark_collected`, ...) the spawn, wait, cancel and
//!   thread paths call.
//!
//! When a parent is idle enough to receive a delivery turn, and the turn itself,
//! stays in [`super::background_delivery`]. The wording is
//! [`super::completion_notice`].

use crate::core::runtime::DispatchCtx;
use std::collections::{HashMap, HashSet, VecDeque};
mod store_adapter;
mod thread_index;
pub(crate) use store_adapter::router_for_workspace_ctx;
pub(super) use store_adapter::{
    degrade_to_memory, entry_for_ctx, open_driver_store, router_pool, router_scope_for,
};
#[cfg(test)]
pub(super) use store_adapter::{
    entry_for, router_for_workspace, router_scope, test_store_overrides,
};
use thread_index::{key, note_thread_workspace};
pub(crate) use thread_index::{
    note_session_thread, router_for_thread, thread_for_session, workspace_for_thread,
};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tinyagents_orchestration::subagent::{
    record_detached_completion, DetachedCompletionTarget, DetachedSubagentStatus,
};
use tinyagents_tasks::{
    CompletionRecord, CompletionResult, CompletionRouter, CompletionState, CompletionStatus,
    CompletionStore, NotifyMode, TombstoneOutcome,
};

use super::completion_notice::BackgroundCompletionFormatter;
pub(crate) use super::completion_notice::{BackgroundAgentOutcome, AWAITING_INPUT_LABEL};
#[cfg(test)]
pub(crate) use super::completion_target::claim_recovery;
#[cfg(test)]
pub(crate) use super::completion_target::tests::{
    forget_workspace_for_test, install_store_for_test, TestWorkspace,
};
pub(crate) use super::completion_target::CompletionTarget;
use super::completion_target::{awaiting_input_record_id, awaiting_input_record_prefix};
pub(crate) use super::completion_target::{claim_recovery_ctx, forget_recovery_ctx};

/// How long a settled record (delivered / gave up / tombstoned) is kept before
/// compaction drops it. Dropping a settled record also drops its dedupe and its
/// tombstone, so this must outlive any child's interest in its parent.
const SETTLED_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How long shutdown waits for a router still in use before dropping it.
const RELEASE_DRAIN: Duration = Duration::from_secs(3);

/// Retries for a failed store write before a completion is reported lost.
const RECORD_RETRIES: u32 = 3;

/// Reserved prefix for independently deliverable pause notices. The encoded
/// task id keeps these records out of the terminal task's dedupe slot; the UUID
/// distinguishes repeated pauses of the same resumable task.
/// Bound on the session -> thread cache; the oldest mapping is evicted first.
const SESSION_THREADS_CAP: usize = 4096;

/// Router authority is scoped to the acting profile and workspace. Threads
/// share a router within that scope, while identical workspace paths in two
/// SaaS profiles remain isolated.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct RouterScope {
    profile_key: String,
    workspace_dir: PathBuf,
}

impl RouterScope {
    #[cfg(test)]
    pub(super) fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }
}

/// A workspace's router plus the store it sits on (kept so boot recovery and
/// purge can enumerate parents, which the router itself does not expose).
pub(super) struct Entry {
    pub(super) router: Arc<CompletionRouter>,
    pub(super) store: Arc<dyn CompletionStore>,
}

#[derive(Default)]
pub(super) struct HostState {
    /// Store sidecars let recovery enumerate per-scope stores without creating
    /// another router cache beside TinyAgents' `CompletionRouterPool`.
    pub(super) stores: HashMap<RouterScope, Arc<dyn CompletionStore>>,
    /// Which workspace holds a thread's completions. Thread-scoped operations
    /// (Stop, delete) carry no workspace, only a thread id.
    pub(super) thread_workspaces: HashMap<String, PathBuf>,
    /// Parent session id -> chat thread id, for the idle gate (busy is tracked
    /// by session; delivery by thread).
    pub(super) session_threads: HashMap<String, String>,
    /// Insertion order of `session_threads`, so the cache evicts its oldest
    /// entry rather than every mapping at once.
    pub(super) session_order: VecDeque<String>,
    /// Threads the user stopped and has not yet re-engaged. Closes the
    /// spawn/register race: a child that registers after Stop is rejected.
    pub(super) stopped_threads: HashSet<String>,
    /// Threads deleted (or purged) in this process. Unlike a stopped thread they
    /// never reopen: a child that registers late, or a straggler that records
    /// after the delete sweep (the cooperative-abort race), is rejected for good.
    pub(super) deleted_threads: HashSet<String>,
    /// Workspaces whose log boot recovery has already scanned this process.
    pub(super) recovered_workspaces: HashSet<RouterScope>,
}

pub(super) fn state() -> std::sync::MutexGuard<'static, HostState> {
    static STATE: OnceLock<Mutex<HostState>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(HostState::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record a finished background sub-agent for idle delivery, keyed by its
/// parent chat thread. Idempotent on `task_id`.
pub(crate) async fn record_completion(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    parent_session: &str,
    task_id: impl Into<String>,
    agent_id: impl Into<String>,
    summary: impl Into<String>,
    parent_thread_id: Option<String>,
) {
    record_outcome(
        ctx,
        workspace_dir,
        parent_session,
        task_id,
        agent_id,
        summary,
        parent_thread_id,
        BackgroundAgentOutcome::Completed,
    )
    .await;
}

/// Record a finished background sub-agent carrying an explicit terminal
/// [`BackgroundAgentOutcome`]. The general enqueue behind
/// [`record_completion`] and the [`record_failure`] / [`record_awaiting_input`]
/// framing helpers, so a failed or awaiting-input async sub-agent is delivered
/// back into chat too — not only successes (#4896).
///
/// The router drops the record when its task was tombstoned (collected inline,
/// stopped) or its thread was cancelled (deleted, stopped).
pub(crate) async fn record_outcome(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    parent_session: &str,
    task_id: impl Into<String>,
    agent_id: impl Into<String>,
    summary: impl Into<String>,
    parent_thread_id: Option<String>,
    outcome: BackgroundAgentOutcome,
) {
    let task_id = task_id.into();
    let Some(thread_id) = parent_thread_id else {
        // Delivery is thread-addressed; a headless spawn has nowhere to land
        // the result. (`spawn_async_subagent` refuses to start one.)
        log::warn!("[background_completions] dropping headless completion task_id={task_id}");
        return;
    };
    if !workspace_dir.is_dir() {
        // The workspace was removed (a data reset) while the child ran; do not
        // recreate it just to log a result nobody can receive.
        log::warn!(
            "[background_completions] dropping completion task_id={task_id}: workspace is gone"
        );
        return;
    }
    {
        // The in-memory gates back up the router's durable cancelled-parent
        // marker, so a failed `cancel_parent` write cannot let a late result in.
        let thread_key = key(ctx, &thread_id);
        let st = state();
        if st.deleted_threads.contains(&thread_key) || st.stopped_threads.contains(&thread_key) {
            log::debug!(
                "[background_completions] dropping completion task_id={task_id} for \
                 stopped/deleted thread_id={thread_id}"
            );
            return;
        }
    }
    if is_marked_deleted(&entry_for_ctx(ctx, workspace_dir), &thread_id) {
        log::debug!(
            "[background_completions] dropping completion task_id={task_id} for deleted \
             thread_id={thread_id}"
        );
        return;
    }
    note_thread_workspace(ctx, &thread_id, workspace_dir);
    note_session_thread(ctx, parent_session, &thread_id);
    super::completion_owners::note(ctx, &[&task_id, parent_session]); // owner, for off-task delivery

    let router = router_for_workspace_ctx(ctx, workspace_dir);
    let summary = summary.into();
    let target = DetachedCompletionTarget::new(router.clone(), thread_id.clone(), agent_id)
        .with_notify_mode(NotifyMode::Followup);
    let status = match outcome {
        BackgroundAgentOutcome::Completed => DetachedSubagentStatus::Completed {
            output: summary,
            iterations: 0,
        },
        BackgroundAgentOutcome::Failed => DetachedSubagentStatus::Failed { error: summary },
        BackgroundAgentOutcome::AwaitingInput => {
            // TinyAgents deliberately excludes pauses from terminal dedupe; the
            // host gives each paused checkpoint its own record identity above.
            let record = CompletionRecord::new(
                task_id.clone(),
                thread_id.clone(),
                target.agent_id.clone(),
                outcome.status(),
                CompletionResult::text(summary),
            )
            .with_notify_mode(NotifyMode::Followup)
            .with_label(AWAITING_INPUT_LABEL);
            record_completion_record(ctx, workspace_dir, &router, record, &task_id, &thread_id)
                .await;
            return;
        }
    };
    if record_detached_completion(&target, &task_id, &status).await {
        log::debug!(
            "[background_completions] recorded task_id={task_id} thread_id={thread_id} outcome={outcome:?}"
        );
    } else {
        // The log keeps refusing writes (disk full, permissions). Keep this
        // process delivering from memory rather than losing the result; what is
        // already on disk replays on the next boot.
        log::error!(
            "[background_completions] store write failed after retries; continuing in memory \
             task_id={task_id} thread_id={thread_id}"
        );
        let fallback = degrade_to_memory(ctx, workspace_dir, &router);
        let fallback_target = DetachedCompletionTarget::new(
            fallback.router.clone(),
            thread_id.clone(),
            target.agent_id.clone(),
        )
        .with_notify_mode(NotifyMode::Followup);
        if !record_detached_completion(&fallback_target, &task_id, &status).await {
            log::error!(
                "[background_completions] completion LOST — in-memory fallback failed task_id={task_id} thread_id={thread_id}"
            );
        }
    }
}

async fn record_completion_record(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    router: &Arc<CompletionRouter>,
    record: CompletionRecord,
    task_id: &str,
    thread_id: &str,
) {
    if let Err(error) = router
        .record_with_retries(record.clone(), RECORD_RETRIES)
        .await
    {
        log::error!(
            "[background_completions] store write failed after retries; continuing in memory task_id={task_id} thread_id={thread_id} error={error}"
        );
        let _ = degrade_to_memory(ctx, workspace_dir, router)
            .router
            .record(record)
            .await;
    }
}

/// Queue a **failed** async sub-agent for chat delivery (#4896). The summary is
/// framed with the `[SUBAGENT_FAILED]` envelope the parent agent is prompted to
/// relay, so the user learns the delegated task errored instead of the turn
/// silently finalizing on "Accepted".
pub(crate) async fn record_failure(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    parent_session: &str,
    task_id: impl Into<String>,
    agent_id: impl Into<String>,
    error: &str,
    parent_thread_id: Option<String>,
) {
    let summary =
        format!("[SUBAGENT_FAILED] the async sub-agent errored before producing a result: {error}");
    record_outcome(
        ctx,
        workspace_dir,
        parent_session,
        task_id,
        agent_id,
        summary,
        parent_thread_id,
        BackgroundAgentOutcome::Failed,
    )
    .await;
}

/// Queue an **awaiting-user** async sub-agent for chat delivery (#4896). A
/// detached child that pauses to ask a question will not continue on its own, so
/// the framed `[SUBAGENT_NEEDS_INPUT]` notice is delivered back into chat for the
/// parent agent to relay to (or answer for) the user. Each pause has a separate
/// router key: the same task may pause repeatedly before its final completion.
pub(crate) async fn record_awaiting_input(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    parent_session: &str,
    task_id: impl Into<String>,
    agent_id: impl Into<String>,
    question: &str,
    checkpointed: bool,
    parent_thread_id: Option<String>,
) {
    let task_id = task_id.into();
    let agent_id = agent_id.into();
    // A pause is deliverable to the parent, but it is not the task's final
    // completion. Use an independent key for every pause so resumed work can
    // pause again and still eventually record under the original task id.
    let pause_record_id = awaiting_input_record_id(&task_id);
    let summary = crate::agent::orchestration::tools::awaiting_user::awaiting_user_envelope(
        &task_id,
        &agent_id,
        None,
        question,
        checkpointed,
    );
    record_outcome(
        ctx,
        workspace_dir,
        parent_session,
        pause_record_id,
        agent_id,
        summary,
        parent_thread_id,
        BackgroundAgentOutcome::AwaitingInput,
    )
    .await;
}

/// Drop every workspace's router and all process-local state about them. Called
/// when the core server stops (`openhuman_rpc::server::serve`), so the completion logs'
/// file handles close before a data reset removes their directory (Windows
/// refuses to delete an open file) and a later boot starts from the logs alone.
/// A router a delivery (or a record) is still using is kept registered until that
/// work lets go, for at most [`RELEASE_DRAIN`], so a reopened log never has two
/// writers; past the deadline the stragglers are dropped with a warning. Returns
/// how many routers were released.
pub(crate) async fn release_all() -> usize {
    let deadline = tokio::time::Instant::now() + RELEASE_DRAIN;
    let mut released = 0;
    loop {
        let scopes: Vec<_> = state().stores.keys().cloned().collect();
        let retained = scopes
            .iter()
            .filter(|scope| {
                router_pool()
                    .get(scope)
                    .ok()
                    .flatten()
                    .is_some_and(|router| Arc::strong_count(&router) > 2)
            })
            .count();
        if retained == 0 {
            let mut st = state();
            released += st.stores.len();
            st.stores.clear();
            if let Err(error) = router_pool().clear() {
                log::warn!("[background_completions] could not clear router pool: {error}");
            }
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            log::warn!(
                "[background_completions] {retained} router(s) still in use at shutdown; \
                 dropping them anyway"
            );
            let mut st = state();
            released += st.stores.len();
            st.stores.clear();
            if let Err(error) = router_pool().clear() {
                log::warn!("[background_completions] could not clear router pool: {error}");
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    {
        // Stop and delete gates outlive the release: a detached child that
        // finishes after shutdown must still be refused for a stopped or deleted
        // thread, and deleted threads never reopen.
        let mut st = state();
        let (stopped, deleted) = (
            std::mem::take(&mut st.stopped_threads),
            std::mem::take(&mut st.deleted_threads),
        );
        *st = HostState {
            stopped_threads: stopped,
            deleted_threads: deleted,
            ..HostState::default()
        };
    }
    log::info!("[background_completions] released {released} router(s) on core shutdown");
    released
}

/// Undelivered completions for `thread_id`, oldest first (read-only; includes
/// records an in-flight delivery currently holds).
#[cfg(test)]
pub(crate) fn pending_for(workspace_dir: &Path, thread_id: &str) -> Vec<CompletionRecord> {
    router_for_workspace(workspace_dir).pending_for(thread_id)
}

pub(crate) fn pending_for_ctx(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    thread_id: &str,
) -> Vec<CompletionRecord> {
    router_for_workspace_ctx(ctx, workspace_dir).pending_for(thread_id)
}

/// Mark `task_id` as collected inline by the parent (via `wait_subagent`) so its
/// background completion is not independently delivered as a second, duplicate
/// answer. The router withdraws an already-recorded completion and drops one
/// that arrives later. Returns whether a pending completion was withdrawn.
#[cfg(test)]
pub(crate) fn mark_collected(workspace_dir: &Path, task_id: &str) -> bool {
    let entry = entry_for(workspace_dir);
    mark_collected_in_entry(entry, task_id)
}

pub(crate) fn mark_collected_ctx(ctx: &DispatchCtx, workspace_dir: &Path, task_id: &str) -> bool {
    mark_collected_in_entry(entry_for_ctx(ctx, workspace_dir), task_id)
}

fn mark_collected_in_entry(entry: Arc<Entry>, task_id: &str) -> bool {
    let router = &entry.router;
    let completion = router.tombstone(task_id);
    // A parent that collected the final result inline has no use for any
    // queued pause notices from that same task. Sweep them too, including
    // pauses persisted before a restart.
    let pause_prefix = awaiting_input_record_prefix(task_id);
    let pauses: Vec<String> = entry
        .store
        .list(None)
        .into_iter()
        .filter(|record| record.task_id.starts_with(&pause_prefix))
        .map(|record| record.task_id)
        .collect();
    let pauses_collected = pauses.into_iter().fold(false, |collected, pause_id| {
        let suppressed = matches!(
            router.tombstone(&pause_id),
            Ok(TombstoneOutcome::Suppressed)
        );
        collected | suppressed
    });
    log::debug!(
        "[background_completions] mark_collected task_id={task_id} completion={completion:?} \
         pauses_collected={pauses_collected}"
    );
    matches!(completion, Ok(TombstoneOutcome::Suppressed)) || pauses_collected
}

/// Drop every queued completion for `thread_id` and everything that finishes
/// for it later. Called when the thread is deleted: the router's cancelled-parent
/// marker is durable, and the thread is also remembered as deleted in memory, so
/// a straggler that wins the cooperative-abort race, a child that registers after
/// the delete, or a result that finishes after a restart is dropped rather than
/// delivered into a thread that no longer exists. Takes the caller's workspace so
/// the marker is written even when no child of the thread was seen by this
/// process. Returns the number of queued completions removed.
pub(crate) fn discard_for_thread(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    thread_id: &str,
) -> usize {
    let seen = {
        let thread_key = key(ctx, thread_id);
        let mut st = state();
        st.deleted_threads.insert(thread_key.clone());
        st.stopped_threads.remove(&thread_key);
        st.thread_workspaces.contains_key(&thread_key)
    };
    let entry = entry_for_ctx(ctx, workspace_dir);
    // Nothing in this process or on disk refers to the thread (an ordinary chat
    // thread that never spawned background work): the in-memory gate is enough
    // and no log line is written for it.
    if !seen && entry.router.pending_for(thread_id).is_empty() {
        return 0;
    }
    let removed = cancel_deleted_parent(&entry, thread_id);
    log::debug!(
        "[background_completions] discard_for_thread thread_id={thread_id} removed={removed}"
    );
    removed
}

/// Task id of the durable "this thread was deleted" marker. The router's
/// cancelled-parent marker cannot tell a delete from a Stop (a Stop is lifted
/// when the user returns), so a delete also leaves this one, which nothing
/// lifts. It is stored `Pending` (with no parent, so nothing ever claims it):
/// compaction only drops settled records, and a deletion must outlive them.
fn deleted_marker_id(thread_id: &str) -> String {
    format!("\u{1}host-thread-deleted:{thread_id}")
}

fn is_marked_deleted(entry: &Entry, thread_id: &str) -> bool {
    entry.store.get(&deleted_marker_id(thread_id)).is_some()
}

/// Cancel `thread_id` for good in `entry`'s router and write the deleted marker.
/// Failures are logged; the in-memory deleted set still gates this process.
fn cancel_deleted_parent(entry: &Entry, thread_id: &str) -> usize {
    let marker = CompletionRecord::new(
        deleted_marker_id(thread_id),
        "",
        "",
        CompletionStatus::Incomplete,
        CompletionResult::default(),
    )
    .with_notify_mode(NotifyMode::Off);
    if let Err(error) = entry.store.put(&marker) {
        log::error!(
            "[background_completions] could not persist the deleted marker thread_id={thread_id} \
             error={error}"
        );
    }
    entry
        .router
        .cancel_parent(thread_id)
        .unwrap_or_else(|error| {
            log::error!(
                "[background_completions] cancel_parent failed thread_id={thread_id} error={error}"
            );
            0
        })
}

/// Drop every queued completion for `thread_id` and gate late results from the
/// stopped generation.
///
/// The Stop-button counterpart of [`discard_for_thread`]: the user halted the
/// thread's work, so results that finished but were not yet delivered must not
/// start a fresh delivery turn behind their back. The thread stays alive; the
/// gate lifts at [`resume_stopped_thread`] (the next accepted user turn), while
/// the stopped generation's task ids stay tombstoned
/// ([`finish_stop_for_thread`]). Returns the number of queued completions removed.
pub(crate) fn discard_pending_for_thread(ctx: &DispatchCtx, thread_id: &str) -> usize {
    state().stopped_threads.insert(key(ctx, thread_id));
    let Some(router) = router_for_thread(ctx, thread_id) else {
        return 0;
    };
    let removed = router.cancel_parent(thread_id).unwrap_or_else(|error| {
        log::error!(
            "[background_completions] cancel_parent failed thread_id={thread_id} error={error}"
        );
        0
    });
    log::debug!(
        "[background_completions] discard_pending_for_thread thread_id={thread_id} removed={removed}"
    );
    removed
}

/// Complete a Stop after its registered children were aborted: tombstone their
/// task ids so a straggler from the stopped generation cannot record once a
/// later user turn reopens the thread.
pub(crate) fn finish_stop_for_thread(ctx: &DispatchCtx, thread_id: &str, task_ids: &[String]) {
    let Some(router) = router_for_thread(ctx, thread_id) else {
        return;
    };
    for task_id in task_ids {
        if let Err(error) = router.tombstone(task_id) {
            log::warn!("[background_completions] tombstone failed task_id={task_id} error={error}");
        }
    }
}

/// Reopen a thread's completion gate for a newly accepted user turn.
///
/// The Stop gate deliberately outlives registry cancellation, because a detached
/// child may be between `tokio::spawn` and `running_subagents::register` when
/// Stop is pressed. New task ids remain distinct from the stopped generation.
/// Only a thread this process stopped or cancelled pays for the durable resume
/// marker, so an ordinary chat message writes nothing.
pub(crate) fn resume_stopped_thread(ctx: &DispatchCtx, thread_id: &str) {
    let was_stopped = state().stopped_threads.remove(&key(ctx, thread_id));
    if !was_stopped {
        return;
    }
    if let Some(router) = router_for_thread(ctx, thread_id) {
        router.resume_parent(thread_id);
        log::debug!("[background_completions] resumed thread_id={thread_id}");
    }
}

/// Record a child that registers while its parent thread is stopped or deleted.
///
/// Registration happens after the detached task is spawned. If Stop (or delete)
/// races that narrow interval the registry sweep cannot see the child;
/// tombstoning its task id here keeps it rejected even after a later user turn
/// reopens a stopped thread. Also notes which workspace holds the thread's
/// completions, so a later thread-scoped Stop can find the router. The first
/// child this process spawns on a (not deleted) thread lifts any
/// cancelled-parent marker an earlier process left behind (a Stop before a
/// restart): a live spawn proves the user re-engaged the thread. Returns whether
/// the child must be aborted.
pub(crate) fn mark_stopped_task_if_thread_stopped(
    ctx: &DispatchCtx,
    workspace_dir: &Path,
    thread_id: &str,
    task_id: &str,
) -> bool {
    let entry = entry_for_ctx(ctx, workspace_dir);
    let thread_key = key(ctx, thread_id);
    let (first_sight, mut stopped) = {
        let mut st = state();
        let first_sight = st
            .thread_workspaces
            .insert(thread_key.clone(), workspace_dir.to_path_buf())
            .is_none();
        let stopped =
            st.stopped_threads.contains(&thread_key) || st.deleted_threads.contains(&thread_key);
        (first_sight, stopped)
    };
    // A delete outlives a restart: the durable marker, not just this process's
    // memory, decides whether a late child belongs to a dead thread.
    if first_sight && !stopped && is_marked_deleted(&entry, thread_id) {
        state().deleted_threads.insert(thread_key);
        stopped = true;
    }
    if stopped {
        if let Err(error) = entry.router.tombstone(task_id) {
            log::warn!("[background_completions] tombstone failed task_id={task_id} error={error}");
        }
        return true;
    }
    if first_sight {
        entry.router.resume_parent(thread_id);
    }
    false
}

/// Withdraw every queued completion of `workspace_dir`. Called on a full thread
/// purge of that workspace; each thread with undelivered results is cancelled
/// durably (and remembered as deleted), so stragglers are still dropped. Other
/// workspaces are untouched. Returns the number of completions removed.
pub(crate) fn clear_all(ctx: &DispatchCtx, workspace_dir: &Path) -> usize {
    let entry = entry_for_ctx(ctx, workspace_dir);
    let parents: HashSet<String> = entry
        .store
        .list(None)
        .into_iter()
        .filter(|r| r.state == CompletionState::Pending && !r.parent_key.is_empty())
        .map(|r| r.parent_key)
        .collect();
    let mut removed = 0;
    for parent in parents {
        state().deleted_threads.insert(key(ctx, &parent));
        removed += cancel_deleted_parent(&entry, &parent);
    }
    log::debug!("[background_completions] clear_all removed={removed}");
    removed
}

/// Boot recovery for a workspace: open its log, drop long-settled records, and
/// return the threads that still hold undelivered completions (a previous
/// process finished the work but never delivered it). Remembers each thread's
/// workspace so the delivery loop can claim them.
pub(crate) fn recover_pending_threads(ctx: &DispatchCtx, workspace_dir: &Path) -> Vec<String> {
    let entry = entry_for_ctx(ctx, workspace_dir);
    if let Err(error) = entry.router.compact(SETTLED_RETENTION) {
        log::warn!("[background_completions] compact failed error={error}");
    }
    let threads: HashSet<String> = entry
        .store
        .list(None)
        .into_iter()
        .filter(|r| {
            r.state == CompletionState::Pending
                && r.notify_mode != NotifyMode::Off
                && !r.parent_key.is_empty()
        })
        .map(|r| r.parent_key)
        .collect();
    let mut threads: Vec<String> = threads
        .into_iter()
        // `pending_for` is the router's view: it drops threads a cancelled-parent
        // marker has since withdrawn.
        .filter(|thread| !entry.router.pending_for(thread).is_empty())
        .collect();
    threads.sort();
    for thread in &threads {
        note_thread_workspace(ctx, thread, workspace_dir);
    }
    log::info!(
        "[background_completions] boot recovery found {} thread(s) with undelivered completions \
         workspace_dir={}",
        threads.len(),
        workspace_dir.display()
    );
    threads
}

#[cfg(test)]
#[path = "background_completions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "background_completions_pause_tests.rs"]
mod pause_tests;

#[cfg(test)]
#[path = "background_completions_release_tests.rs"]
mod release_tests;
