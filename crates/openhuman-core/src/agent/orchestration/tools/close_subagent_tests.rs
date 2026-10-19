use super::*;

#[tokio::test]
async fn close_contract_keeps_required_session_reference_and_requires_live_dispatch() {
    let tool = CloseSubagentTool::new();
    let schema = tool.parameters_schema();
    assert_eq!(schema["required"], json!(["subagent_session_id"]));

    let missing = tool.execute(json!({})).await.unwrap();
    assert!(missing.is_error);
    assert!(missing.output().contains("subagent_session_id"));

    let outside = tool
        .execute(json!({"subagent_session_id":"subsess-1"}))
        .await
        .unwrap();
    assert!(outside.is_error);
    assert!(outside.output().contains("live harness control dispatch"));
}
