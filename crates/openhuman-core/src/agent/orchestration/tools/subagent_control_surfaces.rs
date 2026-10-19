//! OpenHuman declarations and response projection helpers for shared
//! TinyAgents sub-agent controls.
//!
//! Actual query, await, message delivery, and close behavior is dispatched to
//! TinyAgents' Jobs and Message tools by the live host registration. These
//! host tools retain the product names, schemas, metadata, and response
//! envelopes only.

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyagents_orchestration::subagent::SubagentResumeRef;
use tinytools::{PermissionLevel, Tool, ToolResult, ToolTimeout};

use crate::agent::orchestration::subagent_sessions::{
    DurableSubagentSessionSummary, DurableSubagentStatus,
};
use tinyagents_tasks::{OrchestrationTaskRecord, OrchestrationTaskStatus};

pub struct ListSubagentsTool;
pub struct WaitSubagentTool;
pub struct SteerSubagentTool;
pub struct CloseSubagentTool;

impl ListSubagentsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListSubagentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListSubagentsTool {
    fn name(&self) -> &str {
        "list_subagents"
    }

    fn description(&self) -> &str {
        "List active or reusable durable sub-agents owned by the current parent thread."
    }

    fn parameters_schema(&self) -> Value {
        json!({"type":"object", "required":[], "properties":{}})
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::error(
            "list_subagents requires a live harness control dispatch",
        ))
    }
}

impl WaitSubagentTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaitSubagentTool {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 3600;

/// Resolve the product wait deadline used by both the model schema and the
/// harness timeout policy.
pub(crate) fn requested_timeout_secs(args: &Value) -> u64 {
    args.get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS)
}

#[async_trait]
impl Tool for WaitSubagentTool {
    fn name(&self) -> &str {
        "wait_subagent"
    }

    fn description(&self) -> &str {
        "Block until an async sub-agent finishes and return its result. `timeout_secs` defaults to 120, max 3600; timing out is a normal outcome you can retry, so prefer one long wait over tight polling."
    }

    fn timeout_policy(&self, args: &Value) -> ToolTimeout {
        ToolTimeout::Millis(requested_timeout_secs(args).saturating_mul(1000))
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": [],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Transient task_id returned by reusable async delegation."
                },
                "subagent_session_id": {
                    "type": "string",
                    "description": "Durable subagent_session_id returned by reusable async delegation. Preferred for cross-turn waits."
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_TIMEOUT_SECS,
                    "description": "Max seconds to block before returning a 'still running' result. Default 120."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let session_id = args
            .get("subagent_session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if task_id.is_empty() && session_id.is_empty() {
            return Ok(ToolResult::error(
                "wait_subagent: `subagent_session_id` or `task_id` is required",
            ));
        }
        Ok(ToolResult::error(
            "wait_subagent called outside of an agent turn",
        ))
    }
}

impl SteerSubagentTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SteerSubagentTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SteerSubagentTool {
    fn name(&self) -> &str {
        "steer_subagent"
    }

    fn description(&self) -> &str {
        "Send a message into a running async sub-agent, redirecting it or feeding it data mid-run without restarting. It is picked up at the next step. `mode: steer` (default) is an instruction it must address; `mode: collect` is silent context. Returns immediately."
    }

    fn parameters_schema(&self) -> Value {
        crate::tools::schema_cache::static_schema!(include_str!("steer_subagent.json"))
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let task_id = args
            .get("task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let session_id = args
            .get("subagent_session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if task_id.is_empty() && session_id.is_empty() {
            return Ok(ToolResult::error(
                "steer_subagent: `subagent_session_id` or `task_id` is required",
            ));
        }
        if message.is_empty() {
            return Ok(ToolResult::error("steer_subagent: `message` is required"));
        }
        Ok(ToolResult::error(
            "steer_subagent called outside of an agent turn",
        ))
    }
}

impl CloseSubagentTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CloseSubagentTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CloseSubagentTool {
    fn name(&self) -> &str {
        "close_subagent"
    }

    fn description(&self) -> &str {
        "Close a reusable sub-agent session so future delegation creates a fresh worker. If that session is currently running, it is cancelled first."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["subagent_session_id"],
            "properties": {
                "subagent_session_id": {
                    "type": "string",
                    "description": "Durable subagent_session_id returned by reusable async delegation."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        if args
            .get("subagent_session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Ok(ToolResult::error(
                "close_subagent: `subagent_session_id` is required",
            ));
        }
        Ok(ToolResult::error(
            "close_subagent requires a live harness control dispatch",
        ))
    }
}

/// Product status overlay retained for the host's legacy session envelope.
pub(crate) fn overlay_task_store_status(
    summary: &mut DurableSubagentSessionSummary,
    workspace_dir: &std::path::Path,
    parent_session: &str,
) {
    if summary.status == DurableSubagentStatus::Closed {
        return;
    }
    let Some(task_id) = summary.current_task_id.clone() else {
        return;
    };
    let record =
        match crate::agent::orchestration::running_subagents::task_record_for_task_in_workspace(
            workspace_dir,
            &task_id,
            parent_session,
        ) {
            Ok(record) => record,
            Err(tinyagents_orchestration::subagent::WaitError::Unknown) => return,
            Err(tinyagents_orchestration::subagent::WaitError::RegistryPoisoned) => {
                log::warn!(
                    "[subagent_reuse] task store overlay skipped task_id={} reason=registry_poisoned",
                    task_id
                );
                return;
            }
            Err(tinyagents_orchestration::subagent::WaitError::NotOwned) => {
                log::warn!(
                    "[subagent_reuse] task store overlay rejected task_id={} subagent_session_id={} reason=not_owned",
                    task_id,
                    summary.subagent_session_id
                );
                return;
            }
        };
    let previous = summary.status;
    apply_task_record_overlay(summary, record);
    if summary.status != previous {
        log::debug!(
            "[subagent_reuse] overlaid task store status subagent_session_id={} task_id={} previous={:?} status={:?}",
            summary.subagent_session_id,
            task_id,
            previous,
            summary.status
        );
    }
}

fn apply_task_record_overlay(
    summary: &mut DurableSubagentSessionSummary,
    record: OrchestrationTaskRecord,
) {
    match record.status {
        OrchestrationTaskStatus::Pending
        | OrchestrationTaskStatus::Running
        | OrchestrationTaskStatus::CancelRequested => {
            summary.status = DurableSubagentStatus::Running;
            summary.reusable = true;
        }
        OrchestrationTaskStatus::Awaiting => {
            summary.status = DurableSubagentStatus::AwaitingUser;
            summary.reusable = false;
        }
        OrchestrationTaskStatus::Completed => {
            summary.status = DurableSubagentStatus::Idle;
            summary.reusable = true;
            summary.latest_error = None;
        }
        OrchestrationTaskStatus::Failed
        | OrchestrationTaskStatus::TimedOut
        | OrchestrationTaskStatus::Abandoned
        | OrchestrationTaskStatus::Cancelled => {
            summary.status = DurableSubagentStatus::Failed;
            summary.reusable = false;
            summary.latest_error = Some(record.error.unwrap_or_else(|| {
                format!(
                    "sub-agent reached durable task status `{}`",
                    durable_status_label(record.status)
                )
            }));
        }
    }
}

fn durable_status_label(status: OrchestrationTaskStatus) -> &'static str {
    match status {
        OrchestrationTaskStatus::Pending => "pending",
        OrchestrationTaskStatus::Running => "running",
        OrchestrationTaskStatus::Awaiting => "awaiting",
        OrchestrationTaskStatus::Completed => "completed",
        OrchestrationTaskStatus::Failed => "failed",
        OrchestrationTaskStatus::CancelRequested => "cancel_requested",
        OrchestrationTaskStatus::Cancelled => "cancelled",
        OrchestrationTaskStatus::TimedOut => "timed_out",
        OrchestrationTaskStatus::Abandoned => "abandoned",
    }
}

pub(crate) fn format_running_wait_message(
    reference: Option<&SubagentResumeRef>,
    task_id: &str,
    timeout_secs: u64,
) -> String {
    let status = wait_status_payload(
        reference,
        task_id,
        "running",
        None,
        None,
        "continue other work, call wait_subagent again, or call steer_subagent to send more input",
    );
    format!(
        "Sub-agent `{}` is still running after {timeout_secs}s.\n\n[subagent_wait_result]\n{}\n[/subagent_wait_result]\n\nContinue with other work and call wait_subagent again later, or steer_subagent to redirect it.",
        status["agent_id"].as_str().unwrap_or("subagent"),
        serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
    )
}

pub(crate) fn wait_status_payload(
    reference: Option<&SubagentResumeRef>,
    task_id: &str,
    status: &str,
    iterations: Option<usize>,
    detail: Option<&str>,
    next_action: &str,
) -> Value {
    let agent_id = reference.map(|r| r.agent_id.as_str()).unwrap_or("unknown");
    let subagent_session_id = reference.and_then(|r| r.subagent_session_id.as_deref());
    json!({
        "task_id": task_id,
        "taskId": task_id,
        "subagent_session_id": subagent_session_id,
        "subagentSessionId": subagent_session_id,
        "agent_id": agent_id,
        "agentId": agent_id,
        "status": status,
        "iterations": iterations,
        "detail": detail,
        "next_action": next_action,
        "nextAction": next_action,
        "instructions": {
            "send_message": {"tool":"steer_subagent", "arguments":{"subagent_session_id":subagent_session_id,"task_id":task_id,"message":"<message>","mode":"steer"}},
            "wait": {"tool":"wait_subagent", "arguments":{"subagent_session_id":subagent_session_id,"task_id":task_id,"timeout_secs":DEFAULT_TIMEOUT_SECS}},
            "timeout_tick": {"tool":"wait_subagent", "arguments":{"subagent_session_id":subagent_session_id,"task_id":task_id,"timeout_secs":1}}
        }
    })
}

#[cfg(test)]
#[path = "close_subagent_tests.rs"]
mod close_tests;
#[cfg(test)]
#[path = "list_subagents_tests.rs"]
mod list_tests;
#[cfg(test)]
#[path = "steer_subagent_schema_tests.rs"]
mod steer_schema_tests;
#[cfg(test)]
#[path = "steer_subagent_tests.rs"]
mod steer_tests;
#[cfg(test)]
#[path = "wait_subagent_tests.rs"]
mod wait_tests;
