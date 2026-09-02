use super::*;

#[test]
fn all_schemas_are_in_sandbox_namespace() {
    for schema in all_controller_schemas() {
        assert_eq!(schema.namespace, "sandbox");
    }
}

#[test]
fn registered_controllers_match_schemas() {
    let schemas = all_controller_schemas();
    let controllers = all_registered_controllers();
    assert_eq!(schemas.len(), controllers.len());
    for (s, c) in schemas.iter().zip(controllers.iter()) {
        assert_eq!(s.function, c.schema.function);
    }
}

#[tokio::test]
async fn handle_status_returns_json() {
    let result = handle_status(Map::new()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn handle_resolve_policy_none() {
    let mut params = Map::new();
    params.insert("sandbox_mode".into(), Value::String("none".into()));
    let result = handle_resolve_policy(params).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn handle_resolve_policy_sandboxed_remote() {
    let mut params = Map::new();
    params.insert("sandbox_mode".into(), Value::String("sandboxed".into()));
    params.insert("is_remote".into(), Value::Bool(true));
    let result = handle_resolve_policy(params).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    let backend = val.get("backend").and_then(|b| b.as_str());
    assert_eq!(backend, Some("docker"));
}

#[tokio::test]
async fn handle_validate_policy_valid() {
    let policy = super::super::types::SandboxPolicy {
        backend: super::super::types::SandboxBackendKind::Docker,
        workspace_root: std::path::PathBuf::from("/tmp/safe"),
        read_only_mounts: vec![],
        allow_network: false,
        env_passthrough: vec![],
        docker_overrides: None,
    };
    let mut params = Map::new();
    params.insert("policy".into(), serde_json::to_value(&policy).unwrap());
    let result = handle_validate_policy(params).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val.get("valid").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn handle_validate_policy_dangerous() {
    let policy = super::super::types::SandboxPolicy {
        backend: super::super::types::SandboxBackendKind::Docker,
        workspace_root: std::path::PathBuf::from("/"),
        read_only_mounts: vec![],
        allow_network: false,
        env_passthrough: vec![],
        docker_overrides: Some(super::super::types::DockerOverrides {
            network: Some("host".into()),
            ..Default::default()
        }),
    };
    let mut params = Map::new();
    params.insert("policy".into(), serde_json::to_value(&policy).unwrap());
    let result = handle_validate_policy(params).await;
    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(val.get("valid").and_then(|v| v.as_bool()), Some(false));
}
