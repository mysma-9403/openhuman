use super::*;
use crate::openhuman::security::{AutonomyLevel, SecurityPolicy};

use crate::openhuman::memory::api::types::MemoryEntry;

fn test_security() -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy::default())
}

/// Read a memory back through the seam the tool wrote it through.
///
/// The tool holds no handle at all — it resolves the *bound* driver per
/// call — so a fixture store built here could never see the write: under a
/// real module the bound driver is the process-global test workspace, a
/// different store entirely. That is why no fixture handle exists in this
/// module, and why an absence assertion must not be made against one; it
/// would hold whether or not the refusal under test worked, which is worse
/// than no assertion at all.
///
/// The guard is the tool's own door, so a read through it proves the write
/// landed where a caller would look for it.
async fn stored(namespace: &str, key: &str) -> Option<MemoryEntry> {
    active_memory_guard()
        .await
        .expect("a bound memory guard")
        .get(namespace, key)
        .await
        .expect("read back through the guard")
}

#[test]
fn name_and_schema() {
    let tool = MemoryStoreTool::new(test_security());
    assert_eq!(tool.name(), "memory_store");
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["key"].is_object());
    assert!(schema["properties"]["content"].is_object());
    // The memory protocol (#4116) must be stated up front so the model recalls
    // for dedupe before writing and reconciles the index after.
    let desc = tool.description();
    assert!(
        desc.contains("memory_recall") && desc.contains("update_memory_md"),
        "memory_store description must state the read→dedupe→write→update contract: {desc}"
    );
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_core() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool
        .execute(json!({"namespace": "global", "key": "lang", "content": "Prefers Rust"}))
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.output().contains("lang"));

    let entry = stored("global", "lang").await;
    assert!(
        entry.is_some(),
        "the write is visible through the tool's own seam"
    );
    assert_eq!(entry.unwrap().content, "Prefers Rust");
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_with_category() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool
            .execute(
                json!({"namespace": "global", "key": "note", "content": "Fixed bug", "category": "daily"}),
            )
            .await
            .unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_with_custom_category() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool
            .execute(
                json!({"namespace": "global", "key": "proj_note", "content": "Uses async runtime", "category": "project"}),
            )
            .await
            .unwrap();
    assert!(!result.is_error);

    let entry = stored("global", "proj_note")
        .await
        .expect("the stored memory is readable through the guard");
    assert_eq!(entry.content, "Uses async runtime");
    assert_eq!(entry.category, MemoryCategory::Custom("project".into()));
}

/// Regression: a `custom:<name>` wire value (the form `memory_recall` and
/// `Display` now emit) must store as `Custom("<name>")`, not the
/// double-prefixed `Custom("custom:<name>")` — otherwise it would `Display`
/// as `custom:custom:<name>` and stop matching the original category.
#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_strips_custom_prefix_from_wire_category() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool
        .execute(json!({
            "namespace": "global",
            "key": "wire_prefixed_note",
            "content": "Uses async runtime",
            "category": "custom:project"
        }))
        .await
        .unwrap();
    assert!(!result.is_error);

    let entry = stored("global", "wire_prefixed_note")
        .await
        .expect("the stored memory is readable through the guard");
    assert_eq!(
        entry.category,
        MemoryCategory::Custom("project".into()),
        "the `custom:` wire prefix must be stripped, not double-stored"
    );
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_rejects_secret_like_content() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool
        .execute(json!({
            "namespace": "global",
            "key": "api",
            "content": "api_key=sk-123456789012345678901234567890"
        }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("looks like a secret"));
    // Through the guard — see `stored`: an absence assertion against a
    // store the tool never writes to holds whether or not the refusal
    // worked, which makes it worse than no assertion at all.
    assert!(stored("global", "api").await.is_none());
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_missing_key() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool.execute(json!({"content": "no key"})).await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_missing_content() {
    let tool = MemoryStoreTool::new(test_security());
    let result = tool.execute(json!({"key": "no_content"})).await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_blocked_in_readonly_mode() {
    let readonly = Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::ReadOnly,
        ..SecurityPolicy::default()
    });
    let tool = MemoryStoreTool::new(readonly);
    let result = tool
        .execute(json!({"namespace": "global", "key": "readonly_lang", "content": "Prefers Rust"}))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("read-only mode"));
    assert!(stored("global", "readonly_lang").await.is_none());
}

#[tokio::test]
#[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
async fn store_blocked_when_rate_limited() {
    let limited = Arc::new(SecurityPolicy {
        max_actions_per_hour: 0,
        ..SecurityPolicy::default()
    });
    let tool = MemoryStoreTool::new(limited);
    let result = tool
        .execute(
            json!({"namespace": "global", "key": "ratelimited_lang", "content": "Prefers Rust"}),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("Rate limit exceeded"));
    assert!(stored("global", "ratelimited_lang").await.is_none());
}
