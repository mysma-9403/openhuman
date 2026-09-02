use super::*;

#[test]
fn all_schemas_have_namespace() {
    for s in all_controller_schemas() {
        assert_eq!(s.namespace, "tokenjuice");
    }
}

/* Module-backed behavior is covered by TinyJuice's loader E2E. */
#[tokio::test]
#[ignore = "requires a built TinyJuice module"]
async fn detect_handler_classifies_json() {
    let mut p = Map::new();
    p.insert(
        "content".into(),
        Value::String(r#"[{"a":1,"b":2},{"a":3,"b":4}]"#.into()),
    );
    let out = handle_detect(p).await.unwrap();
    assert_eq!(out["kind"], "json");
}

#[tokio::test]
#[ignore = "requires a built TinyJuice module"]
async fn cache_stats_handler_returns_counts() {
    let out = handle_cache_stats(Map::new()).await.unwrap();
    assert!(out["entries"].is_u64());
}
