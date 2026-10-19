//! Blocking-with-timeout collection of a sub-agent's terminal result, with a
//! durable task-store fallback for a sub-agent this process never registered
//! in-memory (e.g. after a core restart).

use std::path::Path;
use std::time::Duration;

use tinyagents_orchestration::subagent::{
    wait_detached, wait_detached_or_durable_with_owners, DetachedSubagentOwners, WaitError,
    WaitOutcome,
};

use super::registry::{admit_workspace, owner_key, registry};

/// Block until `task_id` reaches a terminal status or `timeout` elapses.
pub(crate) async fn wait(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    parent_session: &str,
    timeout: Duration,
) -> Result<WaitOutcome, WaitError> {
    let owner = owner_key(ctx, parent_session)?;
    let Some(context) = ctx.resolved() else {
        return wait_detached(registry(), task_id, &owner, timeout).await;
    };
    let Ok(workspace_dir) = context.workspace_dir() else {
        return wait_detached(registry(), task_id, &owner, timeout).await;
    };
    admit_workspace(ctx, &workspace_dir)?;
    let store = super::resolve::scoped_task_store(ctx, &workspace_dir)?;
    let owners = DetachedSubagentOwners::new(&owner, parent_session);
    wait_detached_or_durable_with_owners(
        registry(),
        store.as_ref(),
        &super::super::background_completions::router_for_workspace_ctx(ctx, &workspace_dir),
        task_id,
        owners,
        timeout,
    )
    .await
}

pub(crate) async fn wait_in_workspace(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
    timeout: Duration,
) -> Result<WaitOutcome, WaitError> {
    admit_workspace(ctx, workspace_dir)?;
    let owner = owner_key(ctx, parent_session)?;
    let store = super::resolve::scoped_task_store(ctx, workspace_dir)?;
    let owners = DetachedSubagentOwners::new(&owner, parent_session);
    wait_detached_or_durable_with_owners(
        registry(),
        store.as_ref(),
        &super::super::background_completions::router_for_workspace_ctx(ctx, workspace_dir),
        task_id,
        owners,
        timeout,
    )
    .await
}
