use super::*;

#[test]
fn catalog_mirrors_builtins() {
    use crate::agent_registry::agents::BUILTINS;

    for b in BUILTINS {
        let expected_uri = format!("openhuman://prompts/agents/{}", b.id);
        assert!(
            RESOURCE_CATALOG.iter().any(|r| r.uri == expected_uri),
            "RESOURCE_CATALOG is missing an entry for built-in agent `{}` \
             (expected URI `{}`). Add it to RESOURCE_CATALOG in resources.rs.",
            b.id,
            expected_uri
        );
    }

    let catalog_agent_count = RESOURCE_CATALOG
        .iter()
        .filter(|r| r.uri.starts_with("openhuman://prompts/agents/"))
        .count();
    assert_eq!(
        catalog_agent_count,
        BUILTINS.len(),
        "RESOURCE_CATALOG has {catalog_agent_count} agent entries but BUILTINS has {}. \
         Remove stale entries from RESOURCE_CATALOG.",
        BUILTINS.len()
    );
}

#[test]
fn list_resources_returns_all_catalog_entries() {
    let resources = resource_specs();
    assert_eq!(
        resources.len(),
        RESOURCE_CATALOG.len(),
        "resources/list count mismatch"
    );
    for entry in &resources {
        assert!(!entry.uri.is_empty(), "uri must be set");
        assert!(!entry.name.is_empty(), "name must be set");
        assert_eq!(entry.mime_type.as_deref(), Some("text/markdown"));
    }
}

#[test]
fn list_resources_includes_core_and_agent_uris() {
    let resources = resource_specs();
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    for expected in [
        "openhuman://prompts/identity",
        "openhuman://prompts/soul",
        "openhuman://prompts/user",
        "openhuman://prompts/agents/orchestrator",
        "openhuman://prompts/agents/planner",
    ] {
        assert!(uris.contains(&expected), "missing URI {expected}");
    }
}

#[test]
fn read_resource_returns_content_for_known_uri() {
    let result = read_resource("openhuman://prompts/identity").expect("should succeed");
    let contents = result["contents"].as_array().expect("contents array");
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], "openhuman://prompts/identity");
    assert_eq!(contents[0]["mimeType"], "text/markdown");
    assert!(!contents[0]["text"].as_str().unwrap_or("").is_empty());
}

#[test]
fn read_resource_returns_minus_32002_for_unknown_uri() {
    let err = read_resource("openhuman://prompts/agents/nonexistent")
        .expect_err("should fail for unknown URI");
    assert_eq!(err.code(), -32002);
    assert!(err.message().contains("nonexistent"));
}

#[test]
fn read_resource_returns_content_for_each_subagent() {
    use crate::agent_registry::agents::BUILTINS;
    for b in BUILTINS {
        let uri = format!("openhuman://prompts/agents/{}", b.id);
        let result = read_resource(&uri)
            .unwrap_or_else(|_| panic!("read_resource failed for agent `{}`", b.id));
        let text = result["contents"][0]["text"].as_str().unwrap_or("");
        assert!(
            !text.is_empty(),
            "prompt content is empty for agent `{}`",
            b.id
        );
    }
}

#[test]
fn all_catalog_uris_are_unique() {
    let mut uris: Vec<&str> = RESOURCE_CATALOG.iter().map(|r| r.uri).collect();
    let original_len = uris.len();
    uris.sort_unstable();
    uris.dedup();
    let deduped_len = uris.len();
    assert_eq!(
        original_len, deduped_len,
        "RESOURCE_CATALOG contains duplicate URIs"
    );
}
