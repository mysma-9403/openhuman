//! Read retired owner terminal formats through the owner's compatibility port.

use super::*;

/// A native document-only task must not create file-era directories or locks.
pub(in crate::agent::subagent_host) fn owner_terminal(
    checkpoint_dir: &std::path::Path,
    key: &SubagentTaskKey,
) -> Result<Option<SubagentOutcome>, SubagentError> {
    if !checkpoint_dir.join("lifecycle").is_dir() {
        return Ok(None);
    }
    tinyagents_orchestration::subagent::FileSubagentPersistence::new(checkpoint_dir)
        .terminal_outcome(key)
}
