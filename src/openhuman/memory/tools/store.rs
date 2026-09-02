use crate::openhuman::memory::api::provider::MemoryCore;
use crate::openhuman::memory::api::types::{MemoryCategory, MemoryTaint};
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::memory::safety;
use crate::openhuman::security::policy::ToolOperation;
use crate::openhuman::security::SecurityPolicy;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Let the agent store memories — its own brain writes
pub struct MemoryStoreTool {
    security: Arc<SecurityPolicy>,
}

impl MemoryStoreTool {
    /// Holds no memory handle — the guarded driver is resolved per call.
    #[must_use]
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }

    fn description(&self) -> &str {
        "Store a general fact or note in an explicit namespace (e.g. global, background, autocomplete, skill-{id}). NOT for preferences — those go to `save_preference`, which writes the store the assistant actually reads. Check `memory_recall` for a near-duplicate first, and call `update_memory_md` afterwards, when you have those tools."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Unique key for this memory (e.g. 'user_lang', 'project_stack')"
                },
                "namespace": {
                    "type": "string",
                    "description": "Target namespace (e.g. 'global', 'background', 'autocomplete', or 'skill-{id}')"
                },
                "content": {
                    "type": "string",
                    "description": "The information to remember"
                },
                "category": {
                    "type": "string",
                    "description": "Memory category: 'core' (permanent), 'daily' (session), 'conversation' (chat), or a custom category name. Defaults to 'core'."
                }
            },
            "required": ["namespace", "key", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let namespace = args
            .get("namespace")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'namespace' parameter"))?;
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let category = match args.get("category").and_then(|v| v.as_str()) {
            Some("core") | None => MemoryCategory::Core,
            Some("daily") => MemoryCategory::Daily,
            Some("conversation") => MemoryCategory::Conversation,
            // Route custom categories through `FromStr` so a `custom:<name>`
            // wire value — the form `memory_recall`/`Display` now emit — resolves
            // back to `Custom("<name>")` instead of `Custom("custom:<name>")`
            // (which would `Display` as `custom:custom:<name>` and stop matching
            // the original category on recall/filter). Legacy bare names still
            // parse to the same `Custom(name)`; an unparseable value falls back
            // to the raw string. (review: prefixed-custom round-trip)
            Some(other) => other
                .parse()
                .unwrap_or_else(|_| MemoryCategory::Custom(other.to_string())),
        };

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "memory_store")
        {
            return Ok(ToolResult::error(error));
        }

        let namespace = namespace.trim();
        if namespace.is_empty() {
            return Ok(ToolResult::error("namespace cannot be empty".to_string()));
        }
        let key = key.trim();
        if key.is_empty() {
            return Ok(ToolResult::error("key cannot be empty".to_string()));
        }

        if safety::has_likely_secret(content) {
            log::warn!(
                "[memory:safety] memory_store rejected secret-like content namespace_chars={} key_chars={} content_chars={}",
                namespace.chars().count(),
                key.chars().count(),
                content.chars().count()
            );
            return Ok(ToolResult::error(
                "Refusing to store content that looks like a secret. Remove credentials or tokens and try again.".to_string(),
            ));
        }

        let display_key = format!("{namespace}/{key}");
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_store: {e}"))?;
        match guard
            .store(
                namespace,
                key,
                content,
                category,
                None,
                // Requested provenance; the guard stamps the effective value.
                MemoryTaint::default(),
            )
            .await
        {
            Ok(()) => Ok(ToolResult::success(format!("Stored memory: {display_key}"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to store memory: {e}"))),
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
