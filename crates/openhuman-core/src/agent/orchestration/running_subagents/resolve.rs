//! Resolving a durable `subagent_session_id` (or a transient `task_id`) back
//! to the live registry entry or, failing that, the durable task-store record
//! — used by tool/control paths that only know the stable session id a
//! sub-agent is addressed by.

use std::path::Path;

use tinyagents_orchestration::subagent::{
    resolve_detached_or_durable_with_owners, resume_ref_for_task as live_resume_ref_for_task,
    task_id_for_session as live_task_id_for_session, DetachedSubagentOwners, SubagentResumeRef,
    WaitError,
};

use super::registry::{admit_workspace, owner_key, registry};
use super::task_ledger::task_store_for_workspace;

/// Resolve a durable `subagent_session_id` to the currently-running transient
/// `task_id`, enforcing parent-session ownership.
pub(crate) fn task_id_for_session(
    ctx: &crate::core::runtime::DispatchCtx,
    subagent_session_id: &str,
    parent_session: &str,
) -> Result<String, WaitError> {
    let owner = owner_key(ctx, parent_session)?;
    let Some(context) = ctx.resolved() else {
        return live_task_id_for_session(registry(), subagent_session_id, &owner);
    };
    let Ok(workspace) = context.workspace_dir() else {
        return live_task_id_for_session(registry(), subagent_session_id, &owner);
    };
    task_id_for_session_in_workspace(ctx, subagent_session_id, parent_session, &workspace)
}

pub(crate) fn task_id_for_session_in_workspace(
    ctx: &crate::core::runtime::DispatchCtx,
    subagent_session_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
) -> Result<String, WaitError> {
    admit_workspace(ctx, workspace_dir)?;
    let owner = owner_key(ctx, parent_session)?;
    let store = scoped_task_store(ctx, workspace_dir)?;
    let owners = DetachedSubagentOwners::new(&owner, parent_session);
    let resolved = resolve_detached_or_durable_with_owners(
        registry(),
        store.as_ref(),
        subagent_session_id,
        owners,
    )?;
    let task_id = resolved.task_id;
    log::debug!(
        "[running_subagents] resolved session from task store subagent_session_id={} task_id={} workspace_dir={}",
        subagent_session_id,
        task_id,
        workspace_dir.display()
    );
    Ok(task_id)
}

pub(crate) fn resume_ref_for_task(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    parent_session: &str,
) -> Result<SubagentResumeRef, WaitError> {
    let owner = owner_key(ctx, parent_session)?;
    let Some(context) = ctx.resolved() else {
        return live_resume_ref_for_task(registry(), task_id, &owner);
    };
    let Ok(workspace) = context.workspace_dir() else {
        return live_resume_ref_for_task(registry(), task_id, &owner);
    };
    resume_ref_for_task_in_workspace(ctx, task_id, parent_session, &workspace)
}

pub(crate) fn resume_ref_for_task_in_workspace(
    ctx: &crate::core::runtime::DispatchCtx,
    task_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
) -> Result<SubagentResumeRef, WaitError> {
    admit_workspace(ctx, workspace_dir)?;
    let owner = owner_key(ctx, parent_session)?;
    let store = scoped_task_store(ctx, workspace_dir)?;
    let owners = DetachedSubagentOwners::new(&owner, parent_session);
    let reference =
        resolve_detached_or_durable_with_owners(registry(), store.as_ref(), task_id, owners)?;
    log::debug!(
        "[running_subagents] resolved resume ref from task store task_id={} workspace_dir={}",
        task_id,
        workspace_dir.display()
    );
    Ok(reference)
}

pub(super) fn scoped_task_store(
    ctx: &crate::core::runtime::DispatchCtx,
    workspace_dir: &Path,
) -> Result<std::sync::Arc<dyn tinyagents_tasks::TaskStore>, WaitError> {
    if let Some(context) = ctx.resolved() {
        return Ok(crate::core::runtime::CoreContext::sync_scope(
            context,
            || task_store_for_workspace(workspace_dir),
        ));
    }
    if crate::core::runtime::is_saas() {
        return Err(WaitError::NotOwned);
    }
    Ok(task_store_for_workspace(workspace_dir))
}
