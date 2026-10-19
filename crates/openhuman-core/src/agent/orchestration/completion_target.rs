//! The per-spawn handle a detached sub-agent records its completion through.
//!
//! Built once at spawn so every terminal path (completed, incomplete, failed,
//! awaiting input) records the same way, in the same workspace router, for the
//! same parent thread. The queue itself is [`super::background_completions`].

use std::path::Path;
use std::path::PathBuf;

use super::background_completions::state;

use super::background_completions::{record_awaiting_input, record_completion, record_failure};

#[cfg(test)]
#[path = "completion_target_tests.rs"]
pub(crate) mod tests;

/// Completion-router key prefix for one task's awaiting-input notices.
pub(super) fn awaiting_input_record_prefix(task_id: &str) -> String {
    let encoded_task_id: String = task_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "{}{}:",
        super::completion_notice::AWAITING_INPUT_RECORD_PREFIX,
        encoded_task_id
    )
}

/// A pause is an independently delivered record, so each pause of a resumed
/// task needs its own key while leaving the terminal task id free.
pub(super) fn awaiting_input_record_id(task_id: &str) -> String {
    format!(
        "{}{}",
        awaiting_input_record_prefix(task_id),
        uuid::Uuid::new_v4().simple()
    )
}

/// Where one detached child's completion lands: its workspace (which router),
/// the parent session (the idle gate) and the parent chat thread (the router's
/// parent key). Built once at spawn so every terminal path records the same way.
#[derive(Clone, Debug)]
pub(crate) struct CompletionTarget {
    /// The dispatch context the child's completions are recorded under.
    ctx: crate::core::runtime::DispatchCtx,
    workspace_dir: PathBuf,
    parent_session: String,
    parent_thread_id: Option<String>,
}

impl CompletionTarget {
    pub(crate) fn new(
        ctx: crate::core::runtime::DispatchCtx,
        workspace_dir: PathBuf,
        parent_session: String,
        parent_thread_id: Option<String>,
    ) -> Self {
        Self {
            ctx,
            workspace_dir,
            parent_session,
            parent_thread_id,
        }
    }

    /// Queue a finished result. See [`record_completion`].
    pub(crate) async fn completed(&self, task_id: &str, agent_id: &str, summary: String) {
        record_completion(
            &self.ctx,
            &self.workspace_dir,
            &self.parent_session,
            task_id,
            agent_id,
            summary,
            self.parent_thread_id.clone(),
        )
        .await;
    }

    /// Queue a failure. See [`record_failure`].
    pub(crate) async fn failed(&self, task_id: &str, agent_id: &str, error: &str) {
        record_failure(
            &self.ctx,
            &self.workspace_dir,
            &self.parent_session,
            task_id,
            agent_id,
            error,
            self.parent_thread_id.clone(),
        )
        .await;
    }

    /// Queue an awaiting-input pause. See [`record_awaiting_input`].
    pub(crate) async fn awaiting_input(
        &self,
        task_id: &str,
        agent_id: &str,
        question: &str,
        checkpointed: bool,
    ) {
        record_awaiting_input(
            &self.ctx,
            &self.workspace_dir,
            &self.parent_session,
            task_id,
            agent_id,
            question,
            checkpointed,
            self.parent_thread_id.clone(),
        )
        .await;
    }
}

/// Claim `workspace_dir`'s boot recovery for this process. `true` exactly once
/// per workspace, so the host can scan every workspace it opens (the bootstrap
/// one, then any other a spawn later opens) without rescanning.
pub(crate) fn claim_recovery_ctx(
    ctx: &crate::core::runtime::DispatchCtx,
    workspace_dir: &Path,
) -> bool {
    state()
        .recovered_workspaces
        .insert(super::background_completions::router_scope_for(
            ctx,
            workspace_dir,
        ))
}

#[cfg(test)]
pub(crate) fn claim_recovery(workspace_dir: &Path) -> bool {
    claim_recovery_ctx(&crate::core::runtime::DispatchCtx::ambient(), workspace_dir)
}

/// Let `workspace_dir` be recovered again (a profile re-leased after release),
/// and reopen its durable store in the cached router before the next scan. The
/// owner migrates records under the router's lease lock, so an active holder
/// can continue through the same router without losing records or leases.
pub(crate) fn forget_recovery_ctx(ctx: &crate::core::runtime::DispatchCtx, workspace_dir: &Path) {
    let scope = super::background_completions::router_scope_for(ctx, workspace_dir);
    state().recovered_workspaces.remove(&scope);
    let Ok(Some(router)) = super::background_completions::router_pool().get(&scope) else {
        return;
    };
    let Ok(store) = super::background_completions::open_driver_store(ctx, workspace_dir) else {
        return;
    };
    let previous = router.store();
    match router.replace_store_if_current(&previous, store) {
        Ok(tinyagents_tasks::CompletionStoreReplaceOutcome::Replaced(store))
        | Ok(tinyagents_tasks::CompletionStoreReplaceOutcome::Current(store)) => {
            state().stores.insert(scope, store);
        }
        Err(error) => {
            log::error!(
                "[background_completions] could not reopen durable completion store workspace_dir={} error={error:#}",
                workspace_dir.display()
            );
        }
    }
}
