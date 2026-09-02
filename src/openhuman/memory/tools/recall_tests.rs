use super::*;
use crate::openhuman::memory::api::provider::MemoryCore;
use crate::openhuman::memory::api::types::MemoryTaint;
use crate::openhuman::memory::MemoryCategory;

/// A namespace nothing else writes to.
///
/// The tool holds no handle — it recalls through the *bound* driver, which
/// under a real module is the process-global test workspace every other
/// test shares. A fixed namespace would make the `Found N` counts below
/// depend on whatever else ran, so each test owns its own.
fn unique_namespace() -> String {
    format!(
        "recall{}",
        &uuid::Uuid::new_v4().as_simple().to_string()[..12]
    )
}

/// Seed through the guard — the door the tool itself recalls through.
///
/// A fixture store built here would be a different store entirely, so
/// seeding into one would leave `recall_finds_match` passing only because
/// it found nothing.
async fn seed(namespace: &str, key: &str, content: &str) {
    active_memory_guard()
        .await
        .expect("a bound memory guard")
        .store(
            namespace,
            key,
            content,
            MemoryCategory::Core,
            None,
            MemoryTaint::default(),
        )
        .await
        .expect("seed through the guard");
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn recall_empty() {
    let namespace = unique_namespace();
    let tool = MemoryRecallTool::new();
    let result = tool
        .execute(json!({"namespace": namespace, "query": "anything"}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("No memories found"));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn recall_finds_match() {
    let namespace = unique_namespace();
    seed(&namespace, "lang", "User prefers Rust").await;
    seed(&namespace, "tz", "Timezone is EST").await;

    let tool = MemoryRecallTool::new();
    let result = tool
        .execute(json!({"namespace": namespace, "query": "Rust"}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("Rust"));
    assert!(result.output().contains("Found 1"));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn recall_respects_limit() {
    let namespace = unique_namespace();
    for i in 0..10 {
        seed(&namespace, &format!("k{i}"), &format!("Rust fact {i}")).await;
    }

    let tool = MemoryRecallTool::new();
    let result = tool
        .execute(json!({"namespace": namespace, "query": "Rust", "limit": 3}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("Found 3"));
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn recall_missing_query() {
    let tool = MemoryRecallTool::new();
    let result = tool.execute(json!({})).await;
    assert!(result.is_err());
}

/// Pure schema assertion — needs no store at all now that the tool holds
/// no handle.
#[test]
fn name_and_schema() {
    let tool = MemoryRecallTool::new();
    assert_eq!(tool.name(), "memory_recall");
    assert!(tool.parameters_schema()["properties"]["query"].is_object());
}
