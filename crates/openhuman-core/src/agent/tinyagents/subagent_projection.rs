//! Thin OpenHuman result projections over TinyAgents Jobs operations.

use std::sync::Arc;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::tool::ToolDispatch;

use super::{OpenHumanRunContext, RegisteredToolDispatch};

#[derive(Clone, Copy)]
pub(super) enum SubagentResultProjection {
    List,
    Wait,
}

pub(super) struct ProjectedSubagentDispatch {
    pub(super) inner: RegisteredToolDispatch,
    pub(super) tool: Arc<dyn tinytools::Tool>,
    pub(super) projection: SubagentResultProjection,
}

#[async_trait::async_trait]
impl ToolDispatch<(), OpenHumanRunContext> for ProjectedSubagentDispatch {
    fn tool(&self) -> Arc<dyn tinytools::Tool> {
        self.tool.clone()
    }

    fn output_origin(&self) -> tinyagents_harness::host::ContentOrigin {
        self.inner.output_origin()
    }

    async fn execute(
        &self,
        state: &(),
        call_id: tinyagents_harness::CallId,
        arguments: serde_json::Value,
        options: tinytools::ToolCallOptions,
        parent: &RunContext<OpenHumanRunContext>,
    ) -> anyhow::Result<tinytools::ToolResult> {
        let result = self
            .inner
            .execute(state, call_id, arguments.clone(), options, parent)
            .await?;
        Ok(project_subagent_result(
            self.projection,
            result,
            &arguments,
            parent,
        ))
    }
}

fn project_subagent_result(
    projection: SubagentResultProjection,
    result: tinytools::ToolResult,
    arguments: &serde_json::Value,
    parent: &RunContext<OpenHumanRunContext>,
) -> tinytools::ToolResult {
    match projection {
        SubagentResultProjection::List => project_list_subagents(result, parent),
        SubagentResultProjection::Wait => project_wait_subagent(result, arguments, parent),
    }
}

fn project_list_subagents(
    result: tinytools::ToolResult,
    parent: &RunContext<OpenHumanRunContext>,
) -> tinytools::ToolResult {
    use tinytools::{ToolContent, ToolResult};

    if result.is_error {
        return result;
    }
    let Some(ToolContent::Json { data }) = result.content.first() else {
        return result;
    };
    let Some(rows) = data.as_array() else {
        return result;
    };
    let Some(parent_ctx) = parent.data.parent.as_ref() else {
        return result;
    };
    let mut summaries = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.get("kind").and_then(serde_json::Value::as_str) == Some("session"))
    {
        let mut row = row.clone();
        if let Some(object) = row.as_object_mut() {
            object.remove("kind");
        }
        let Ok(mut summary) = serde_json::from_value::<
            crate::agent::orchestration::subagent_sessions::DurableSubagentSessionSummary,
        >(row) else {
            continue;
        };
        crate::agent::orchestration::tools::overlay_task_store_status(
            &mut summary,
            &parent_ctx.workspace_dir,
            &parent_ctx.session_id,
        );
        summaries.push(summary);
    }
    ToolResult::success(format!(
        "[subagent_sessions]\n{}\n[/subagent_sessions]",
        serde_json::to_string_pretty(&summaries).unwrap_or_else(|_| "[]".into())
    ))
}

fn project_wait_subagent(
    result: tinytools::ToolResult,
    arguments: &serde_json::Value,
    parent: &RunContext<OpenHumanRunContext>,
) -> tinytools::ToolResult {
    use tinyagents_orchestration::subagent::SubagentResumeRef;
    use tinytools::{ToolContent, ToolResult};

    if result.is_error {
        return result;
    }
    let Some(ToolContent::Json { data }) = result.content.first() else {
        return result;
    };
    let Some(object) = data.as_object() else {
        return result;
    };
    let Some(parent_ctx) = parent.data.parent.as_ref() else {
        return result;
    };
    let extensions = object
        .get("extensions")
        .and_then(serde_json::Value::as_object);
    let task_id = extensions
        .and_then(|values| values.get("task_id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("task_id").and_then(serde_json::Value::as_str))
        .or_else(|| arguments.get("task_id").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_owned();
    let agent_id = extensions
        .and_then(|values| values.get("agent_id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("agent").and_then(serde_json::Value::as_str))
        .unwrap_or("unknown")
        .to_owned();
    if task_id.is_empty() {
        return result;
    }
    let parent_thread = parent.data.thread_id.as_deref();
    let store = crate::agent::orchestration::subagent_sessions::SubagentSessionStore::new(
        parent_ctx.workspace_dir.clone(),
    );
    let session_id = crate::agent::orchestration::subagent_sessions::list_for_parent(
        &store,
        &parent_ctx.session_id,
        parent_thread,
    )
    .ok()
    .and_then(|sessions| {
        sessions
            .into_iter()
            .find(|session| session.current_task_id.as_deref() == Some(&task_id))
            .map(|session| session.subagent_session_id)
    });
    let resume = SubagentResumeRef {
        task_id: task_id.clone(),
        agent_id: agent_id.clone(),
        subagent_session_id: session_id,
    };
    let status = object
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("running");
    let iterations = extensions
        .and_then(|values| values.get("iterations"))
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize);
    let output = object
        .get("output")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let error = object
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("subagent execution failed");

    match status {
        "completed" => {
            crate::agent::orchestration::background_completions::mark_collected_ctx(
                &parent.data.dispatch_ctx(),
                &parent_ctx.workspace_dir,
                &task_id,
            );
            let payload = crate::agent::orchestration::tools::wait_status_payload(
                Some(&resume),
                &task_id,
                "completed",
                iterations,
                None,
                "synthesize the sub-agent output into the parent response",
            );
            ToolResult::success(format!(
                "Sub-agent `{agent_id}` completed in {} iteration(s).\n\n[subagent_wait_result]\n{}\n[/subagent_wait_result]\n\n{output}",
                iterations.unwrap_or_default(),
                serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
            ))
        }
        "awaiting_input" | "awaiting_user" => {
            let question = output;
            let payload = crate::agent::orchestration::tools::wait_status_payload(
                Some(&resume),
                &task_id,
                "awaiting_user",
                None,
                Some(question),
                "ask the user for the missing information, then call continue_subagent",
            );
            let mut message = format!(
                "Sub-agent `{agent_id}` paused for clarification and did not finish: {question}\n\nIt cannot proceed unattended. Resume it with continue_subagent once you have an answer.\n\n[subagent_wait_result]\n{}\n[/subagent_wait_result]",
                serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
            );
            if let Some(session_id) = resume.subagent_session_id {
                message.push_str("\n\n[subagent_resume_ref]\n");
                message.push_str(
                    &serde_json::to_string(&serde_json::json!({
                        "task_id": task_id,
                        "agent_id": agent_id,
                        "subagent_session_id": session_id,
                        "tool": "continue_subagent"
                    }))
                    .unwrap_or_else(|_| "{}".into()),
                );
                message.push_str("\n[/subagent_resume_ref]");
            }
            ToolResult::success(message)
        }
        "failed" | "incomplete" | "cancelled" => {
            let payload = crate::agent::orchestration::tools::wait_status_payload(
                Some(&resume),
                &task_id,
                "failed",
                None,
                Some(error),
                "report the failure or retry with a corrected instruction",
            );
            ToolResult::error(format!(
                "Sub-agent `{agent_id}` failed: {error}\n\n[subagent_wait_result]\n{}\n[/subagent_wait_result]",
                serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
            ))
        }
        _ => ToolResult::success(
            crate::agent::orchestration::tools::format_running_wait_message(
                Some(&resume),
                &task_id,
                crate::agent::orchestration::tools::requested_timeout_secs(arguments),
            ),
        ),
    }
}
