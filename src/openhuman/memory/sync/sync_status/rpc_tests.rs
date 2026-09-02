use super::*;

#[test]
fn response_keeps_top_level_statuses_array() {
    let value = serde_json::to_value(StatusListResponse {
        statuses: Vec::new(),
    })
    .unwrap();
    assert!(value
        .get("statuses")
        .is_some_and(serde_json::Value::is_array));
}
