use super::*;

#[test]
fn all_controller_schemas_lists_registered_functions() {
    let schemas = all_controller_schemas();
    assert_eq!(schemas.len(), 9);
    assert!(schemas
        .iter()
        .any(|schema| schema.namespace == "session_db"));
    assert!(schemas
        .iter()
        .any(|schema| schema.namespace == "run_ledger"));
}

#[test]
fn all_registered_controllers_match_schemas() {
    let registered = all_registered_controllers();
    let schemas = all_controller_schemas();
    assert_eq!(registered.len(), schemas.len());

    let schema_fns: Vec<&str> = schemas.iter().map(|s| s.function).collect();
    for rc in &registered {
        assert!(
            schema_fns.contains(&rc.schema.function),
            "registered controller '{}' not in schema list",
            rc.schema.function
        );
    }
}

#[test]
fn schema_for_list_has_optional_inputs() {
    let s = schema_for("session_db_list");
    assert_eq!(s.function, "list");
    assert!(s.inputs.iter().all(|i| !i.required));
}

#[test]
fn schema_for_get_requires_id() {
    let s = schema_for("session_db_get");
    assert_eq!(s.function, "get");
    assert_eq!(s.inputs.len(), 1);
    assert!(s.inputs[0].required);
    assert_eq!(s.inputs[0].name, "id");
}

#[test]
fn schema_for_search_has_query_and_filters() {
    let s = schema_for("session_db_search");
    assert_eq!(s.function, "search");
    let names: Vec<&str> = s.inputs.iter().map(|i| i.name).collect();
    assert!(names.contains(&"query"));
    assert!(names.contains(&"agentId"));
    assert!(names.contains(&"toolName"));
    assert!(names.contains(&"sourceChannel"));
    assert!(names.contains(&"threadId"));
}

#[test]
fn schema_for_unknown_returns_error_shape() {
    let s = schema_for("session_db_nonexistent");
    assert_eq!(s.function, "unknown");
}

#[test]
fn schema_for_run_ledger_events_requires_run_id() {
    let s = schema_for("run_ledger_events");
    assert_eq!(s.namespace, "run_ledger");
    assert_eq!(s.function, "events");
    assert!(s
        .inputs
        .iter()
        .any(|input| input.name == "runId" && input.required));
}

#[test]
fn new_correlation_id_is_eight_hex_chars() {
    let cid = new_correlation_id();
    assert_eq!(cid.len(), 8);
    assert!(cid.chars().all(|c| c.is_ascii_hexdigit()));
}
