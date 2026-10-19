//! Run-ledger persistence and error reporting for web-chat progress events.

use crate::storage::sessions::with_ledger;

pub(in crate::web_chat) fn ledger_upsert_agent_run(
    config: &crate::config::Config,
    upsert: tinyagents_session::run_ledger::AgentRunUpsert,
) {
    if let Err(err) = with_ledger(&config.workspace_dir, |ledger| async move {
        ledger.upsert_agent_run(upsert).await
    }) {
        log::warn!("[run_ledger][web_channel] failed to upsert run: {err}");
    }
}

pub(in crate::web_chat) fn ledger_append_event(
    config: &crate::config::Config,
    event: tinyagents_session::run_ledger::RunEventAppend,
) {
    if let Err(err) = with_ledger(&config.workspace_dir, |ledger| async move {
        ledger.append_run_event(event).await
    }) {
        log::warn!("[run_ledger][web_channel] failed to append event: {err}");
    }
}

pub(in crate::web_chat) fn ledger_upsert_telemetry(
    config: &crate::config::Config,
    telemetry: tinyagents_session::run_ledger::RunTelemetryUpsert,
) {
    if let Err(err) = with_ledger(&config.workspace_dir, |ledger| async move {
        ledger.upsert_run_telemetry(telemetry).await
    }) {
        log::warn!("[run_ledger][web_channel] failed to upsert telemetry: {err}");
    }
}
