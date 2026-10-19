use super::*;

pub(crate) fn decode_driver_job_result(
    tool_result: tinytools::ToolResult,
    bundle: &OpenHumanDriverBundle,
    task_key: &SubagentTaskKey,
) -> Result<String, SubagentRunError> {
    let (public_id, status) = decode_driver_job_reference(tool_result, bundle, task_key)?;
    if let Some(status) = status {
        let expected_status = bundle
            .host_outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(task_key)
            .and_then(|outcome| neutral_status_for_host(&outcome.status));
        if expected_status != Some(status.as_str()) {
            return Err(SubagentRunError::Provider(anyhow::anyhow!(
                "driver-backed async spawn failed: lifecycle status did not match its host outcome"
            )));
        }
    }
    Ok(public_id)
}

pub(crate) async fn resolve_inline_driver_outcome(
    tool_result: tinytools::ToolResult,
    bundle: &OpenHumanDriverBundle,
    task_key: &SubagentTaskKey,
    fallback_agent_id: &str,
) -> Result<(SubagentRunOutcome, String), SubagentRunError> {
    let (public_job_id, declared_status) =
        decode_driver_job_reference(tool_result, bundle, task_key)?;
    if let Some(error) = bundle_failure_for_key(bundle, task_key) {
        return Err(error);
    }
    let cached = bundle
        .host_outcome
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(task_key);

    // Observer cancellation belongs to this waiter, not to the persisted
    // child outcome. It must remain authoritative even if the leader commits
    // while this caller is projecting its ToolResult.
    if let Some(outcome) = cached.as_ref().filter(|host| {
        host.persistence_disposition == SubagentPersistenceDisposition::ObserverCancelled
    }) {
        return Ok((outcome.clone(), public_job_id));
    }

    // The driver's persistence CAS result is authoritative. Another
    // invocation can win terminal/pause persistence while this executor's
    // host-side cache still contains its local loser. Read the same scoped
    // record the driver returned before projecting a host result.
    let persisted = if let Some(outcome) = bundle
        .persistence
        .load_terminal(task_key)
        .await
        .map_err(map_lifecycle_error)?
    {
        Some((
            outcome,
            cached
                .as_ref()
                .map_or(SubagentPersistenceDisposition::TerminalExisting, |host| {
                    host.persistence_disposition
                }),
        ))
    } else {
        bundle
            .persistence
            .load_pause(task_key)
            .await
            .map_err(map_lifecycle_error)?
            .map(|outcome| {
                (
                    outcome,
                    cached
                        .as_ref()
                        .map_or(SubagentPersistenceDisposition::PauseExisting, |host| {
                            host.persistence_disposition
                        }),
                )
            })
    };

    if let Some((outcome, disposition)) = persisted {
        if declared_status
            .as_deref()
            .is_some_and(|status| neutral_status_for_outcome(&outcome.status) != Some(status))
        {
            return Err(SubagentRunError::Provider(anyhow::anyhow!(
                "driver-backed async spawn failed: lifecycle status did not match its durable outcome"
            )));
        }
        let host = outcome_to_host(
            SubagentRunResult::new(outcome, disposition),
            cached,
            &bundle.persistence,
            task_key,
            fallback_agent_id,
        );
        return Ok((host, public_job_id));
    }

    Err(SubagentRunError::Provider(anyhow::anyhow!(
        "driver-backed subagent returned without a durable host outcome"
    )))
}

fn decode_driver_job_reference(
    tool_result: tinytools::ToolResult,
    bundle: &OpenHumanDriverBundle,
    task_key: &SubagentTaskKey,
) -> Result<(String, Option<String>), SubagentRunError> {
    if tool_result.is_error {
        if let Some(error) = bundle_failure_for_key(bundle, task_key) {
            return Err(error);
        }
    }
    let (reply, status) = match tool_result.content.first() {
        Some(tinytools::ToolContent::Json { data }) if !tool_result.is_error => {
            (data.clone(), None)
        }
        Some(tinytools::ToolContent::Text { text }) if tool_result.is_error => {
            let reply: serde_json::Value = serde_json::from_str(text).map_err(|_| {
                SubagentRunError::Provider(anyhow::anyhow!(format!(
                    "driver-backed async spawn failed: {}",
                    tool_result.output()
                )))
            })?;
            let status = reply
                .get("status")
                .and_then(serde_json::Value::as_str)
                .filter(|status| matches!(*status, "awaiting_input" | "incomplete" | "cancelled"))
                .map(str::to_owned)
                .ok_or_else(|| {
                    SubagentRunError::Provider(anyhow::anyhow!(format!(
                        "driver-backed async spawn failed: {}",
                        tool_result.output()
                    )))
                })?;
            (reply, Some(status))
        }
        _ => {
            return Err(SubagentRunError::Provider(anyhow::anyhow!(format!(
                "driver-backed async spawn returned no structured job reference: {}",
                tool_result.output()
            ))));
        }
    };
    let public_id = reply
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .filter(|task_id| !task_id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            SubagentRunError::Provider(anyhow::anyhow!(
                "driver-backed async spawn omitted its registered job id"
            ))
        })?;
    Ok((public_id, status))
}

fn neutral_status_for_host(status: &SubagentRunStatus) -> Option<&'static str> {
    match status {
        SubagentRunStatus::AwaitingUser { .. } => Some("awaiting_input"),
        SubagentRunStatus::Incomplete { .. } => Some("incomplete"),
        SubagentRunStatus::Cancelled => Some("cancelled"),
        SubagentRunStatus::Completed => None,
    }
}

fn neutral_status_for_outcome(status: &SubagentOutcomeKind) -> Option<&'static str> {
    match status {
        SubagentOutcomeKind::AwaitingInput(_) => Some("awaiting_input"),
        SubagentOutcomeKind::Incomplete(_) => Some("incomplete"),
        SubagentOutcomeKind::Cancelled => Some("cancelled"),
        SubagentOutcomeKind::Completed => None,
    }
}

fn bundle_failure_for_key(
    bundle: &OpenHumanDriverBundle,
    task_key: &SubagentTaskKey,
) -> Option<SubagentRunError> {
    bundle
        .failures
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(task_key)
}
