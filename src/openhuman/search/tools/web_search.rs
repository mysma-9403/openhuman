use super::{SearchResponse, SearchResultItem, SeltzSearchTool};
use crate::openhuman::integrations::IntegrationClient;
use crate::openhuman::tools::traits::{Tool, ToolCallOptions, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Provider the OpenHuman Managed search path resolves to today. Exa powers
/// the overwhelming majority of managed search traffic, so it is the labelled
/// default whenever the backend response does not name a provider. This is a
/// *fallback* label, not a hardcoded one: [`resolve_managed_provider`] prefers
/// the provider the backend actually reports, so a future routing change flows
/// through to the UI attribution ("Searched with …", #5136) with no code edit.
const MANAGED_DEFAULT_PROVIDER: &str = "Exa";

/// Resolve the provider name to attribute a managed search to. Uses the
/// backend-reported provider when present and non-empty, otherwise falls back
/// to [`MANAGED_DEFAULT_PROVIDER`]. Shared with the `tools.web_search` RPC so
/// both managed-search surfaces attribute a call the same way.
pub(crate) fn resolve_managed_provider(resp: &SearchResponse) -> &str {
    resp.provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or(MANAGED_DEFAULT_PROVIDER)
}

/// Web search tool backed by the server-side Parallel integration proxy.
pub struct WebSearchTool {
    client: Option<Arc<IntegrationClient>>,
    direct_search: Option<SeltzSearchTool>,
    max_results: usize,
    timeout_secs: u64,
}

impl WebSearchTool {
    pub fn new(
        client: Option<Arc<IntegrationClient>>,
        max_results: usize,
        timeout_secs: u64,
    ) -> Self {
        Self {
            client,
            direct_search: None,
            max_results: max_results.clamp(1, 10),
            timeout_secs: timeout_secs.max(1),
        }
    }

    pub fn with_direct_search(mut self, direct_search: Option<SeltzSearchTool>) -> Self {
        self.direct_search = direct_search;
        self
    }

    fn parse_parallel_results(
        &self,
        results: &[SearchResultItem],
        query: &str,
        provider: &str,
    ) -> anyhow::Result<String> {
        if results.is_empty() {
            // Still attribute an empty search: the call completed, so the
            // timeline must not keep showing it as in-progress (#5136).
            return Ok(format!(
                "No results found for: {} (via {})",
                query, provider
            ));
        }

        let mut lines = vec![format!("Search results for: {} (via {})", query, provider)];

        for (i, result) in results.iter().take(self.max_results).enumerate() {
            let title = if result.title.trim().is_empty() {
                "No title"
            } else {
                result.title.trim()
            };
            let url = result.url.trim();

            lines.push(format!("{}. {}", i + 1, title));
            lines.push(format!("   {}", url));

            if let Some(date) = result.publish_date.as_deref() {
                let date = date.trim();
                if !date.is_empty() {
                    lines.push(format!("   Published: {}", date));
                }
            }

            if let Some(first) = result.excerpts.first() {
                let excerpt = first.trim();
                if !excerpt.is_empty() {
                    let truncated = crate::openhuman::util::truncate_with_ellipsis(excerpt, 500);
                    lines.push(format!("   {}", truncated));
                }
            }
        }

        Ok(lines.join("\n"))
    }

    fn render_results_markdown(
        &self,
        results: &[SearchResultItem],
        query: &str,
        provider: &str,
    ) -> String {
        if results.is_empty() {
            return format!("_No results for `{query}`_ (via {provider})");
        }
        let mut out = format!("# Search results — `{query}` (via {provider})\n");
        for r in results.iter().take(self.max_results) {
            let title = if r.title.trim().is_empty() {
                "Untitled"
            } else {
                r.title.trim()
            };
            out.push_str(&format!("\n## [{title}]({})\n", r.url.trim()));
            if let Some(date) = r.publish_date.as_deref() {
                let date = date.trim();
                if !date.is_empty() {
                    out.push_str(&format!("_Published: {date}_\n\n"));
                }
            }
            if let Some(first) = r.excerpts.first() {
                let excerpt = first.trim();
                if !excerpt.is_empty() {
                    let truncated = crate::openhuman::util::truncate_with_suffix(excerpt, 500, "…");
                    out.push_str(&format!("> {truncated}\n"));
                }
            }
        }
        out
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search_tool"
    }

    fn description(&self) -> &str {
        "Search the web for information via a configured direct search API or the backend search proxy. Returns relevant search results with titles, URLs, and descriptions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query. Be specific for better results."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute_with_options(
        &self,
        args: serde_json::Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: query"))?;

        if query.trim().is_empty() {
            anyhow::bail!("Search query cannot be empty");
        }
        let query = query.trim().to_string();

        if let Some(direct_search) = &self.direct_search {
            tracing::debug!(
                query_len = query.chars().count(),
                max_results = self.max_results,
                timeout_secs = self.timeout_secs,
                "[web_search] direct Seltz search"
            );
            let mut normalized_args = args;
            if let Some(obj) = normalized_args.as_object_mut() {
                obj.insert("query".to_string(), Value::String(query.clone()));
            }
            return direct_search
                .execute_with_options(normalized_args, options)
                .await;
        }

        let client = self.client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Web search unavailable: no backend session token. Sign in first so the server can proxy search."
            )
        })?;

        let query_fingerprint = hex::encode(Sha256::digest(query.as_bytes()));
        tracing::debug!(
            query_len = query.chars().count(),
            query_fingerprint = %query_fingerprint[..16],
            max_results = self.max_results,
            timeout_secs = self.timeout_secs,
            "[web_search] backend parallel search"
        );

        // Body matches `parallelSearchSchema` in backend-2. The legacy
        // `numResults` / `maxCharactersPerExcerpt` aliases still work, but
        // current fields are `maxResults` / `maxCharsPerResult`. Also dropping
        // `timeoutSecs` — the validator does not declare it and Parallel's
        // per-mode deadlines drive timing on the upstream side.
        let _ = self.timeout_secs;
        let body = json!({
            "objective": query.clone(),
            "searchQueries": [query.clone()],
            "mode": "fast",
            "excerpts": {
                "maxResults": self.max_results,
                "maxCharsPerResult": 500
            }
        });

        let resp = client
            .post::<SearchResponse>("/agent-integrations/parallel/search", &body)
            .await?;

        // Attribute the search to the provider the managed backend resolved to
        // (Exa by default). The provider name is echoed in the result text so
        // the UI can surface "Searched with <provider>" without a hardcode.
        let provider = resolve_managed_provider(&resp);
        tracing::debug!(provider, "[web_search] managed search resolved provider");

        let mut result =
            ToolResult::success(self.parse_parallel_results(&resp.results, &query, provider)?);
        if options.prefer_markdown {
            result.markdown_formatted =
                Some(self.render_results_markdown(&resp.results, &query, provider));
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "web_search_tests.rs"]
mod tests;
