use super::*;
use crate::openhuman::tools::traits::ToolScope;

fn cfg() -> Arc<Config> {
    Arc::new(Config::default())
}

#[test]
fn names_and_levels() {
    let c = cfg();
    assert_eq!(
        TaskSourceListTool::new(c.clone()).name(),
        "task_source_list"
    );
    assert_eq!(
        TaskSourceListTool::new(c.clone()).permission_level(),
        PermissionLevel::ReadOnly
    );
    assert_eq!(
        TaskSourceFetchTool::new(c.clone()).permission_level(),
        PermissionLevel::Execute
    );
    assert_eq!(
        TaskSourceAddTool::new(c.clone()).permission_level(),
        PermissionLevel::Write
    );
    assert_eq!(
        TaskSourceRemoveTool::new(c.clone()).permission_level(),
        PermissionLevel::Dangerous
    );
    assert_eq!(TaskSourceListTool::new(c).scope(), ToolScope::All);
}

#[test]
fn read_tools_concurrency_safe() {
    let c = cfg();
    assert!(TaskSourceListTool::new(c.clone()).is_concurrency_safe(&serde_json::Value::Null));
    assert!(TaskSourceGetTool::new(c).is_concurrency_safe(&serde_json::Value::Null));
}

#[tokio::test]
async fn get_requires_id() {
    let err = TaskSourceGetTool::new(cfg())
        .execute(json!({}))
        .await
        .expect_err("missing id");
    assert!(err.to_string().contains("id"));
}

#[tokio::test]
async fn add_requires_provider_and_filter() {
    let err = TaskSourceAddTool::new(cfg())
        .execute(json!({ "filter": { "provider": "github" } }))
        .await
        .expect_err("missing provider");
    assert!(err.to_string().contains("provider"));
}

#[test]
fn parse_provider_rejects_unknown() {
    let err = parse_provider(&json!({ "provider": "jira" })).expect_err("unknown provider");
    assert!(err.to_string().contains("provider"));
}
