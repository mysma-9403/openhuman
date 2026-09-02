use super::*;
#[tokio::test]
#[ignore = "requires a built TinyJuice module"]
async fn retrieves_offloaded_original() {
    let original = "ORIGINAL PAYLOAD ".repeat(20);
    let hash = "module-fixture";
    let tool = RetrieveToolOutputTool::new();
    let res = tool.execute(json!({ "hash": hash })).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(res.output(), original);
}

#[tokio::test]
async fn missing_hash_is_error() {
    let tool = RetrieveToolOutputTool::new();
    let res = tool
        .execute(json!({ "hash": "deadbeefcafe" }))
        .await
        .unwrap();
    assert!(res.is_error);
    let res2 = tool.execute(json!({})).await.unwrap();
    assert!(res2.is_error);
}
