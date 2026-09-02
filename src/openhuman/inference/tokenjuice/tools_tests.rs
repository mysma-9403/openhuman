use super::*;
#[tokio::test]
#[ignore = "requires a built TinyJuice module"]
async fn retrieves_offloaded_original() {
    let original = "ORIGINAL TOKENJUICE PAYLOAD ".repeat(20);
    let hash = "module-fixture";
    let tool = TokenjuiceRetrieveTool::new();
    let res = tool.execute(json!({ "token": hash })).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(res.output(), original);
}

#[tokio::test]
#[ignore = "requires a built TinyJuice module"]
async fn retrieves_line_range() {
    let _original = "r0\nr1\nr2\nr3\nr4";
    let hash = "module-fixture";
    let tool = TokenjuiceRetrieveTool::new();
    let res = tool
        .execute(json!({ "token": hash, "range": { "start": 1, "end": 3, "unit": "lines" } }))
        .await
        .unwrap();
    assert!(!res.is_error);
    assert_eq!(res.output(), "r1\nr2");
}

#[tokio::test]
async fn missing_token_is_error() {
    let tool = TokenjuiceRetrieveTool::new();
    let res = tool
        .execute(json!({ "token": "deadbeefcafe" }))
        .await
        .unwrap();
    assert!(res.is_error);
    let res2 = tool.execute(json!({})).await.unwrap();
    assert!(res2.is_error);
}
