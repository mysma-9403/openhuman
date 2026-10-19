use super::*;

#[tokio::test]
async fn validation_errors_name_the_delegate_graph_tool() {
    let tool: Arc<dyn Tool> = Arc::new(DelegateGraphTool::new());
    let dispatch = DelegateGraphDispatch::new(tool);
    let parent = crate::agent::tinyagents::host::OpenHumanRunContext::new().into_tinyagents(
        tinyagents_harness::context::RunConfig::new("delegate-graph-test"),
    );

    let result = dispatch
        .execute(
            &(),
            tinyagents_harness::CallId::new("delegate-graph-test-call"),
            serde_json::json!({"task": "test validation"}),
            ToolCallOptions::default(),
            &parent,
        )
        .await
        .expect("validation returns a tool result");

    assert!(result.is_error);
    assert_eq!(result.output(), "delegate_graph: `agent_id` is required.");
}
