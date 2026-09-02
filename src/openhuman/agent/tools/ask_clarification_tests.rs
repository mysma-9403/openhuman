use super::*;
use serde_json::json;

#[test]
fn name_is_correct() {
    assert_eq!(AskClarificationTool::new().name(), "ask_user_clarification");
}

#[test]
fn description_is_non_empty() {
    assert!(!AskClarificationTool::new().description().is_empty());
}

#[test]
fn schema_is_object_type() {
    let schema = AskClarificationTool::new().parameters_schema();
    assert_eq!(schema["type"], "object");
}

#[test]
fn permission_level_is_none() {
    assert_eq!(
        AskClarificationTool::new().permission_level(),
        PermissionLevel::None
    );
}

#[test]
fn default_and_new_are_equivalent() {
    let a = AskClarificationTool::new();
    let b = AskClarificationTool::default();
    assert_eq!(a.name(), b.name());
}

#[tokio::test]
async fn execute_with_question_includes_question_in_output() {
    let tool = AskClarificationTool::new();
    let result = tool
        .execute(json!({ "question": "Which branch should I target?" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("Which branch should I target?"));
}

#[tokio::test]
async fn execute_with_options_lists_choices() {
    let tool = AskClarificationTool::new();
    let result = tool
        .execute(json!({
            "question": "Which env?",
            "options": ["staging", "production"]
        }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let out = result.output();
    assert!(out.contains("staging"));
    assert!(out.contains("production"));
}

#[tokio::test]
async fn execute_without_question_uses_fallback() {
    let tool = AskClarificationTool::new();
    let result = tool.execute(json!({})).await.unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("CLARIFICATION NEEDED"));
}
