use super::*;

#[test]
fn catalog_lists_all_five_controllers() {
    let schemas = all_controller_schemas();
    assert_eq!(schemas.len(), 5);
    let names: Vec<&str> = schemas.iter().map(|s| s.function).collect();
    assert!(names.contains(&"connect"));
    assert!(names.contains(&"disconnect"));
    assert!(names.contains(&"state"));
    assert!(names.contains(&"emit"));
    assert!(names.contains(&"connect_with_session"));
}

#[test]
fn registered_controllers_match_schemas_count() {
    let schemas = all_controller_schemas();
    let handlers = all_registered_controllers();
    assert_eq!(schemas.len(), handlers.len());
}

#[test]
fn all_schemas_use_socket_namespace() {
    for s in all_controller_schemas() {
        assert_eq!(s.namespace, "socket", "function {}", s.function);
        assert!(
            !s.description.is_empty(),
            "function {} has empty description",
            s.function
        );
    }
}

#[test]
fn connect_schema_requires_url_and_token() {
    let s = schemas("connect");
    let required: Vec<&str> = s
        .inputs
        .iter()
        .filter(|f| f.required)
        .map(|f| f.name)
        .collect();
    assert!(required.contains(&"url"));
    assert!(required.contains(&"token"));
}

#[test]
fn disconnect_and_state_have_no_inputs() {
    assert!(schemas("disconnect").inputs.is_empty());
    assert!(schemas("state").inputs.is_empty());
    assert!(schemas("connect_with_session").inputs.is_empty());
}

#[test]
fn emit_schema_data_is_optional() {
    let s = schemas("emit");
    let event = s.inputs.iter().find(|f| f.name == "event").unwrap();
    let data = s.inputs.iter().find(|f| f.name == "data").unwrap();
    assert!(event.required);
    assert!(!data.required);
}

#[test]
fn unknown_function_returns_unknown_fallback_schema() {
    let s = schemas("no_such_fn");
    assert_eq!(s.namespace, "socket");
    assert_eq!(s.function, "unknown");
    assert_eq!(s.outputs.len(), 1);
    assert_eq!(s.outputs[0].name, "error");
}

#[test]
fn every_schema_has_at_least_one_output_field() {
    for s in all_controller_schemas() {
        assert!(
            !s.outputs.is_empty(),
            "schema `{}` must expose ≥1 output field for RPC callers",
            s.function
        );
    }
}

#[test]
fn all_registered_controllers_have_socket_namespace() {
    for h in all_registered_controllers() {
        assert_eq!(h.schema.namespace, "socket");
        assert!(!h.schema.function.is_empty());
    }
}

#[test]
fn connect_schema_inputs_contain_url_and_token() {
    let s = schemas("connect");
    let names: Vec<&str> = s.inputs.iter().map(|f| f.name).collect();
    assert!(names.contains(&"url"));
    assert!(names.contains(&"token"));
}

// ── handlers (without manager): require_manager errors ─────────

#[tokio::test]
async fn handlers_error_without_initialized_manager() {
    // Production bootstrap calls `set_global_socket_manager` once; in
    // these unit tests the global singleton is intentionally NOT set,
    // so every handler should hit the `SocketManager not initialized`
    // branch via `require_manager()` first.
    //
    // We can't reliably clear a OnceLock once set. If another test in
    // the same binary has already installed a global manager, skip
    // rather than cross-contaminating.
    if super::global_socket_manager().is_some() {
        eprintln!(
            "[socket:schemas tests] global manager already installed — \
             skipping require_manager error-path assertions"
        );
        return;
    }

    let err = handle_disconnect(Map::new()).await.unwrap_err();
    assert!(err.contains("SocketManager not initialized"));

    let err = handle_state(Map::new()).await.unwrap_err();
    assert!(err.contains("SocketManager not initialized"));

    let err = handle_connect(Map::new()).await.unwrap_err();
    assert!(err.contains("SocketManager not initialized"));

    let err = handle_emit(Map::new()).await.unwrap_err();
    assert!(err.contains("SocketManager not initialized"));
}
