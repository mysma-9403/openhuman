use super::*;
use serde_json::json;

#[test]
fn all_controller_schemas_covers_every_function() {
    let names: Vec<_> = all_controller_schemas()
        .into_iter()
        .map(|s| s.function)
        .collect();
    assert_eq!(
        names,
        vec![
            "list",
            "get",
            "add",
            "update",
            "remove",
            "fetch",
            "sync",
            "list_tasks",
            "preview_filter",
            "list_databases",
            "status"
        ]
    );
}

#[test]
fn all_registered_controllers_has_handler_per_schema() {
    let controllers = all_registered_controllers();
    assert_eq!(controllers.len(), all_controller_schemas().len());
    assert!(controllers
        .iter()
        .all(|c| c.schema.namespace == "task_sources"));
}

#[test]
fn schemas_add_requires_provider_and_filter() {
    let s = schemas("add");
    let names: Vec<_> = s.inputs.iter().map(|f| f.name).collect();
    assert!(names.contains(&"provider"));
    assert!(names.contains(&"filter"));
    let provider = s.inputs.iter().find(|f| f.name == "provider").unwrap();
    assert!(provider.required);
}

#[test]
fn schemas_unknown_function_returns_placeholder() {
    let s = schemas("nope");
    assert_eq!(s.function, "unknown");
    assert_eq!(s.outputs[0].name, "error");
}

#[test]
fn read_provider_parses_known_and_rejects_unknown() {
    let mut params = Map::new();
    params.insert("provider".into(), json!("notion"));
    assert_eq!(read_provider(&params).unwrap(), ProviderSlug::Notion);

    params.insert("provider".into(), json!("jira"));
    assert!(read_provider(&params).is_err());
}

#[test]
fn read_optional_handles_absent_null_and_value() {
    let mut params = Map::new();
    assert_eq!(read_optional::<u64>(&params, "limit").unwrap(), None);
    params.insert("limit".into(), Value::Null);
    assert_eq!(read_optional::<u64>(&params, "limit").unwrap(), None);
    params.insert("limit".into(), json!(7));
    assert_eq!(read_optional::<u64>(&params, "limit").unwrap(), Some(7));
}
