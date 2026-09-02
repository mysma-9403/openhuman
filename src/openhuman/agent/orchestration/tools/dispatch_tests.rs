use super::*;
use crate::openhuman::tools::traits::Tool;

use crate::openhuman::agent::tools::AskClarificationTool;

#[test]
fn ask_clarification_tool_re_exported() {
    let tool = AskClarificationTool::new();
    assert_eq!(tool.name(), "ask_user_clarification");
}

#[tokio::test]
async fn dispatch_subagent_returns_tool_error_when_agent_unknown() {
    // Exercises the graceful-failure paths of `dispatch_subagent`:
    // without a global registry we get the "registry not initialised"
    // branch, and with one (set by another test in the same binary)
    // a bogus agent id hits the "agent not found" branch. Either way
    // the function must return `Ok(ToolResult::error(..))` rather than
    // panicking or returning `Err`.
    let res = dispatch_subagent(
        "__definitely_not_a_real_agent__",
        "test_tool",
        "irrelevant prompt",
        None,
        None,
        None,
        DispatchMode::Blocking,
    )
    .await
    .expect("dispatch_subagent should not return Err on these inputs");

    assert!(res.is_error, "expected a tool-error ToolResult");
    let out = res.output();
    assert!(
        out.contains("registry not initialised") || out.contains("not found in registry"),
        "unexpected graceful-failure message: {out}"
    );
}

#[test]
fn awaiting_user_outcome_maps_to_resume_envelope_not_bare_success() {
    // #4291: a delegated sub-agent that pauses on `ask_user_clarification`
    // must come back as the `[SUBAGENT_AWAITING_USER]` envelope (so the
    // orchestrator resumes via continue_subagent) — NOT a plain success
    // carrying the question as if the task were done, which made the
    // orchestrator re-spawn a fresh mcp_setup and loop.
    use crate::openhuman::agent::harness::subagent_runner::{
        SubagentMode, SubagentRunOutcome, SubagentRunStatus, SubagentUsage,
    };
    use std::time::Duration;

    let question = "Which MCP server would you like to install?".to_string();
    let outcome = SubagentRunOutcome {
        task_id: "sub-xyz789".to_string(),
        agent_id: "mcp_setup".to_string(),
        output: String::new(),
        iterations: 1,
        elapsed: Duration::from_secs(0),
        mode: SubagentMode::Typed,
        status: SubagentRunStatus::AwaitingUser {
            question: question.clone(),
            options: None,
        },
        final_history: Vec::new(),
        usage: SubagentUsage::default(),
        artifact_paths: Vec::new(),
    };

    let res = awaiting_outcome_to_tool_result(&outcome, &question);
    assert!(!res.is_error, "awaiting-user is not a failure");
    let out = res.output();
    assert!(out.contains("[SUBAGENT_AWAITING_USER]"), "envelope: {out}");
    assert!(out.contains("task_id: sub-xyz789"), "envelope: {out}");
    assert!(out.contains("agent_id: mcp_setup"), "envelope: {out}");
    assert!(out.contains("continue_subagent"), "envelope: {out}");
    assert!(
        out.contains(&question),
        "envelope must carry question: {out}"
    );
}

#[test]
fn subagent_failure_envelope_forbids_fabricated_success() {
    // #3193: a hard delegation failure (e.g. run_code's coding model
    // 404ing) must be surfaced so the orchestrator cannot narrate a
    // plausible success. The envelope states the task did not run, tells
    // the model not to fabricate output, and preserves the root error.
    let msg = format_subagent_failure(
        "run_code",
        "openhuman API error (404): model 'davinci-002' does not support \
         the chat-completions API",
    );
    assert!(msg.contains("run_code failed"), "names the tool: {msg}");
    assert!(
        msg.contains("did not complete"),
        "states no completion: {msg}"
    );
    assert!(
        msg.to_lowercase().contains("do not treat this as success") && msg.contains("fabricate"),
        "warns against fabricated success: {msg}"
    );
    assert!(
        msg.contains("davinci-002") && msg.contains("404"),
        "preserves the root error: {msg}"
    );
}
