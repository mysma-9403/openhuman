//! Recovery after an unclean hand-over: when a profile's lease is taken
//! over from a holder that never released it, that holder's turns may have
//! died mid-flight. [`ProfileHost::open`](super::ProfileHost::open) runs
//! these sweeps before the profile opens.

use std::sync::Arc;

use super::types::ProfileId;
use crate::core::runtime::CoreContext;

/// Settle what a previous holder left in profile `id`'s workspace: turns that
/// were mid-flight become interrupted and run-ledger rows left running are
/// closed. The sweep a single-user core runs at boot, run when a profile's
/// lease is taken over from a holder that never released it. Failures are
/// logged; the profile still opens.
pub(crate) fn recover_workspace(id: &ProfileId, workspace_dir: &std::path::Path) {
    let now = chrono::Utc::now().to_rfc3339();
    match tinyagents_session::turn_state::store::mark_all_interrupted(
        workspace_dir.to_path_buf(),
        &now,
    ) {
        Ok(0) => {}
        Ok(turns) => log::info!("[profiles] profile={id} recovered {turns} interrupted turn(s)"),
        Err(error) => log::warn!("[profiles] profile={id} turn recovery failed: {error}"),
    }
    match tinyagents_session::run_ledger::interrupt_orphaned_agent_runs(workspace_dir) {
        Ok(0) => {}
        Ok(runs) => log::info!("[profiles] profile={id} settled {runs} orphaned run(s)"),
        Err(error) => log::warn!("[profiles] profile={id} run recovery failed: {error:#}"),
    }
}

/// [`recover_workspace`] for turn states kept in a storage-backed session
/// store, which the workspace sweep cannot see: the profile's default
/// session key is swept through the installed provider.
pub(crate) fn recover_session_store(id: &ProfileId, context: &Arc<CoreContext>) {
    let Some(provider) = crate::agent::session_store::installed() else {
        return;
    };
    if provider.workspace_dir().is_some() {
        // File-backed: the workspace sweep already covered it.
        return;
    }
    let key = crate::core::runtime::session_key(&crate::core::runtime::Tenant::of(context));
    let now = chrono::Utc::now().to_rfc3339();
    match provider
        .for_agent(&key)
        .turn_states
        .mark_all_interrupted(&now)
    {
        Ok(0) => {}
        Ok(turns) => {
            log::info!("[profiles] profile={id} recovered {turns} interrupted stored turn(s)");
        }
        Err(error) => log::warn!("[profiles] profile={id} stored turn recovery failed: {error}"),
    }
}
