use super::*;
use crate::openhuman::tools::ToolResult;
use async_trait::async_trait;
use tinyagents_harness::testkit::ScriptedModel;
use tinyinference::message::AssistantMessage;
use tinyinference::model::{ChatModel, ModelProfile, ModelResponse};
use tinyinference::tool::ToolCall;

struct PingTool;
#[async_trait]
impl Tool for PingTool {
    fn name(&self) -> &str {
        "ping"
    }
    fn description(&self) -> &str {
        "ping"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _a: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("pong"))
    }
}

#[tokio::test]
async fn channel_turn_runs_through_the_graph() {
    let registry: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![Box::new(PingTool)]);
    let mut history = vec![ChatMessage::user("ping please")];
    let scripted: Arc<dyn ChatModel<()>> = Arc::new(ScriptedModel::new(vec![
        ModelResponse {
            message: AssistantMessage {
                id: None,
                content: Vec::new(),
                tool_calls: vec![ToolCall::new("p", "ping", serde_json::json!({}))],
                usage: None,
            },
            usage: None,
            finish_reason: Some("tool_calls".to_string()),
            raw: None,
            resolved_model: None,
            continue_turn: None,
            served_from_cache: false,
        },
        ModelResponse::assistant("channel done"),
    ]));
    let mut profile = ModelProfile::default();
    profile.tool_calling = true;
    profile.parallel_tool_calls = true;
    let text = run_channel_turn_via_graph(
        TurnModelSource::from_model_with_profile(scripted, profile),
        &mut history,
        registry,
        vec![],
        None,
        "mock-model",
        0.0,
        10,
        MultimodalConfig::default(),
        MultimodalFileConfig::default(),
        None,
    )
    .await
    .expect("channel graph turn runs");
    assert_eq!(text, "channel done");
    assert!(history.iter().any(|m| m.content.contains("pong")));
}
