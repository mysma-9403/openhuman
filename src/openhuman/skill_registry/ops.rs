//! Business logic for the skill registry: fetch catalogs, search, and install.

use async_trait::async_trait;
use futures::stream::StreamExt;
use serde::Deserialize;

use super::store;
use super::types::{CatalogEntry, RegistryKind, RegistrySource};

const MAX_CATALOG_BYTES: usize = 5 * 1024 * 1024;
const FETCH_TIMEOUT_SECS: u64 = 30;

/// Bound on how many registry sources are fetched concurrently.
///
/// The enabled-source set is tiny (3 bundled defaults + any custom), so this
/// only needs to be large enough to overlap the typical handful of GitHub raw
/// round-trips without opening an unbounded number of sockets.
const CATALOG_FETCH_CONCURRENCY: usize = 4;

/// Default registry sources shipped with the app.
pub fn default_sources() -> Vec<RegistrySource> {
    vec![
        RegistrySource {
            id: "openhuman-community".into(),
            name: "OpenHuman Community Skills".into(),
            url: "https://raw.githubusercontent.com/tinyhumansai/skill-registry/main/index.json"
                .into(),
            kind: RegistryKind::GithubIndex,
            enabled: true,
        },
        RegistrySource {
            id: "awesome-openclaw".into(),
            name: "OpenClaw Skills".into(),
            url: "https://raw.githubusercontent.com/VoltAgent/awesome-openclaw-skills/main/index.json"
                .into(),
            kind: RegistryKind::GithubIndex,
            enabled: true,
        },
        RegistrySource {
            id: "hermes-community".into(),
            name: "Hermes Community Skills".into(),
            url: "https://raw.githubusercontent.com/hermes-agent/skill-index/main/index.json"
                .into(),
            kind: RegistryKind::GithubIndex,
            enabled: true,
        },
    ]
}

/// Resolve the active list of registry sources: defaults + any user-added custom ones.
pub fn list_sources() -> Vec<RegistrySource> {
    let mut sources = default_sources();
    let custom = store::load_custom_sources();
    tracing::debug!(
        default_count = sources.len(),
        custom_count = custom.len(),
        "[skill_registry] list_sources"
    );
    for c in custom {
        if !sources.iter().any(|s| s.id == c.id) {
            sources.push(c);
        }
    }
    tracing::debug!(total = sources.len(), "[skill_registry] list_sources done");
    sources
}

/// Add a custom registry source.
pub fn add_source(source: RegistrySource) -> Result<(), String> {
    tracing::debug!(
        source_id = %source.id,
        source_url = %source.url,
        "[skill_registry] add_source"
    );
    if source.id.is_empty() {
        return Err("source id must not be empty".into());
    }
    if source.url.is_empty() {
        return Err("source url must not be empty".into());
    }
    let mut custom = store::load_custom_sources();
    if custom.iter().any(|s| s.id == source.id) {
        return Err(format!("source '{}' already exists", source.id));
    }
    custom.push(source);
    store::save_custom_sources(&custom);
    store::clear_cache();
    tracing::info!("[skill_registry] source added, cache cleared");
    Ok(())
}

/// Remove a custom registry source by id.
pub fn remove_source(id: &str) -> Result<(), String> {
    tracing::debug!(source_id = %id, "[skill_registry] remove_source");
    let mut custom = store::load_custom_sources();
    let before = custom.len();
    custom.retain(|s| s.id != id);
    if custom.len() == before {
        return Err(format!("source '{id}' not found in custom sources"));
    }
    store::save_custom_sources(&custom);
    store::clear_cache();
    tracing::info!(source_id = %id, "[skill_registry] source removed, cache cleared");
    Ok(())
}

/// Narrow seam over the concrete `reqwest`-backed catalog fetch so the
/// multi-source fan-out can be unit-tested with a fake fetcher (no network).
#[async_trait]
trait CatalogFetcher {
    async fn fetch(&self, source: &RegistrySource) -> Result<Vec<CatalogEntry>, String>;
}

/// Real fetcher: one HTTP GET + parse per source via `fetch_source_catalog`.
struct HttpCatalogFetcher {
    client: reqwest::Client,
}

#[async_trait]
impl CatalogFetcher for HttpCatalogFetcher {
    async fn fetch(&self, source: &RegistrySource) -> Result<Vec<CatalogEntry>, String> {
        fetch_source_catalog(&self.client, source).await
    }
}

/// Fetch every enabled source with bounded concurrency, preserving the exact
/// serial-loop result: `buffered(K)` yields per-source results in input order,
/// so the accumulated entries and the diagnostic `errors` list are identical to
/// the old one-at-a-time loop — only the wall-clock latency drops (from the sum
/// of all fetches to roughly `ceil(N / K)` round-trips).
async fn collect_catalogs(
    fetcher: &(dyn CatalogFetcher + Sync),
    enabled: &[&RegistrySource],
) -> (Vec<CatalogEntry>, Vec<String>) {
    // Materialize the per-source futures into a `Vec` before `stream::iter` — a
    // lazy `Map` adaptor trips the "implementation of `Send` is not general
    // enough" HRTB error.
    let fetch_futs: Vec<_> = enabled
        .iter()
        .map(|source| async move { (source.id.clone(), fetcher.fetch(source).await) })
        .collect();

    let results: Vec<(String, Result<Vec<CatalogEntry>, String>)> =
        futures::stream::iter(fetch_futs)
            .buffered(CATALOG_FETCH_CONCURRENCY)
            .collect()
            .await;

    let mut all_entries: Vec<CatalogEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (source_id, result) in results {
        match result {
            Ok(entries) => {
                tracing::debug!(
                    source = %source_id,
                    count = entries.len(),
                    "[skill_registry] fetched catalog"
                );
                all_entries.extend(entries);
            }
            Err(e) => {
                tracing::warn!(
                    source = %source_id,
                    error = %e,
                    "[skill_registry] failed to fetch catalog"
                );
                errors.push(format!("{source_id}: {e}"));
            }
        }
    }
    (all_entries, errors)
}

/// Fetch the full catalog from all enabled sources, using cache when fresh.
pub async fn browse_catalog(force_refresh: bool) -> Result<Vec<CatalogEntry>, String> {
    if !force_refresh {
        if let Some(cached) = store::load_cached_catalog() {
            return Ok(cached);
        }
    }

    let sources = list_sources();
    let enabled: Vec<&RegistrySource> = sources.iter().filter(|s| s.enabled).collect();

    if enabled.is_empty() {
        return Ok(Vec::new());
    }

    tracing::info!(
        count = enabled.len(),
        "[skill_registry] fetching catalogs from {} source(s)",
        enabled.len()
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let fetcher = HttpCatalogFetcher { client };
    let (mut all_entries, errors) = collect_catalogs(&fetcher, &enabled).await;

    all_entries.sort_by(|a, b| {
        b.stars
            .unwrap_or(0)
            .cmp(&a.stars.unwrap_or(0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    store::save_catalog_cache(&all_entries);

    if all_entries.is_empty() && !errors.is_empty() {
        return Err(format!(
            "all registry sources failed: {}",
            errors.join("; ")
        ));
    }

    Ok(all_entries)
}

/// Search the catalog by query string, matching against name, description, tags, and format.
pub async fn search_catalog(
    query: &str,
    format_filter: Option<&str>,
    source_filter: Option<&str>,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        query = %query,
        format_filter = ?format_filter,
        source_filter = ?source_filter,
        "[skill_registry] search_catalog"
    );
    let catalog = browse_catalog(false).await?;
    let q = query.to_lowercase();

    let filtered: Vec<CatalogEntry> = catalog
        .into_iter()
        .filter(|entry| {
            if let Some(fmt) = format_filter {
                if entry.format != fmt {
                    return false;
                }
            }
            if let Some(src) = source_filter {
                if entry.source_id != src {
                    return false;
                }
            }
            if q.is_empty() {
                return true;
            }
            entry.name.to_lowercase().contains(&q)
                || entry.description.to_lowercase().contains(&q)
                || entry.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || entry.format.to_lowercase().contains(&q)
                || entry
                    .author
                    .as_deref()
                    .map(|a| a.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();

    tracing::debug!(
        result_count = filtered.len(),
        "[skill_registry] search_catalog complete"
    );
    Ok(filtered)
}

/// Install a skill from the catalog by its entry. Delegates to the existing
/// `install_workflow_from_url` in the workflows module.
pub async fn install_from_catalog(
    workspace_dir: &std::path::Path,
    entry: &CatalogEntry,
) -> Result<crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlOutcome, String> {
    tracing::info!(
        entry_id = %entry.id,
        source = %entry.source_id,
        format = %entry.format,
        "[skill_registry] installing from catalog"
    );

    let params = crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlParams {
        url: entry.download_url.clone(),
        timeout_secs: Some(60),
    };

    crate::openhuman::workflows::ops_install::install_workflow_from_url(workspace_dir, params).await
}

async fn fetch_source_catalog(
    client: &reqwest::Client,
    source: &RegistrySource,
) -> Result<Vec<CatalogEntry>, String> {
    let response = client
        .get(&source.url)
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "fetch returned status {}",
            response.status().as_u16()
        ));
    }

    if let Some(len) = response.content_length() {
        if len > MAX_CATALOG_BYTES as u64 {
            return Err(format!("catalog too large: {len} bytes"));
        }
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read body: {e}"))?;

    if body.len() > MAX_CATALOG_BYTES {
        return Err(format!("catalog too large: {} bytes", body.len()));
    }

    match source.kind {
        RegistryKind::GithubIndex | RegistryKind::HttpCatalog => {
            parse_index_json(&body, &source.id)
        }
    }
}

/// Parse a JSON index file. Expects either:
/// - An array of entry objects directly
/// - An object with a "skills" or "entries" array field
fn parse_index_json(body: &str, source_id: &str) -> Result<Vec<CatalogEntry>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;

    let entries_value = if value.is_array() {
        value
    } else if let Some(arr) = value.get("skills").or(value.get("entries")) {
        arr.clone()
    } else {
        return Err("index JSON must be an array or contain a 'skills'/'entries' field".into());
    };

    let raw_entries: Vec<RawIndexEntry> = serde_json::from_value(entries_value)
        .map_err(|e| format!("failed to parse index entries: {e}"))?;

    let entries = raw_entries
        .into_iter()
        .filter_map(|raw| {
            let name = raw.name.as_deref().or(raw.title.as_deref())?.to_string();
            let description = raw
                .description
                .as_deref()
                .or(raw.summary.as_deref())
                .unwrap_or("")
                .to_string();
            let id = raw
                .id
                .as_deref()
                .or(raw.slug.as_deref())
                .unwrap_or(&name)
                .to_string();

            let download_url = raw
                .download_url
                .as_deref()
                .or(raw.url.as_deref())
                .or(raw.raw_url.as_deref())?
                .to_string();

            let format = raw
                .format
                .as_deref()
                .or(raw.skill_format.as_deref())
                .unwrap_or("openhuman")
                .to_string();

            Some(CatalogEntry {
                id,
                name,
                description,
                format,
                author: raw.author,
                version: raw.version,
                tags: raw.tags.unwrap_or_default(),
                download_url,
                source_id: source_id.to_string(),
                stars: raw.stars.or(raw.star_count),
                updated_at: raw.updated_at.or(raw.last_updated),
            })
        })
        .collect();

    Ok(entries)
}

/// Flexible raw index entry that handles varying field names across registries.
#[derive(Debug, Deserialize)]
struct RawIndexEntry {
    id: Option<String>,
    slug: Option<String>,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    summary: Option<String>,
    format: Option<String>,
    skill_format: Option<String>,
    author: Option<String>,
    version: Option<String>,
    tags: Option<Vec<String>>,
    download_url: Option<String>,
    url: Option<String>,
    raw_url: Option<String>,
    stars: Option<u32>,
    star_count: Option<u32>,
    updated_at: Option<String>,
    last_updated: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn source(id: &str) -> RegistrySource {
        RegistrySource {
            id: id.to_string(),
            name: id.to_string(),
            url: format!("https://example.test/{id}.json"),
            kind: RegistryKind::GithubIndex,
            enabled: true,
        }
    }

    fn entry(id: &str, source_id: &str) -> CatalogEntry {
        CatalogEntry {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            format: "openhuman".to_string(),
            author: None,
            version: None,
            tags: Vec::new(),
            download_url: format!("https://example.test/{id}.md"),
            source_id: source_id.to_string(),
            stars: None,
            updated_at: None,
        }
    }

    /// Fake fetcher returning canned per-source results, tracking the observed
    /// max in-flight concurrency so a test can assert the fan-out really overlaps.
    struct FakeFetcher {
        responses: HashMap<String, Result<Vec<CatalogEntry>, String>>,
        inflight: Arc<AtomicUsize>,
        max_inflight: Arc<AtomicUsize>,
        delay_ms: u64,
    }

    impl FakeFetcher {
        fn new(delay_ms: u64) -> Self {
            Self {
                responses: HashMap::new(),
                inflight: Arc::new(AtomicUsize::new(0)),
                max_inflight: Arc::new(AtomicUsize::new(0)),
                delay_ms,
            }
        }

        fn with(mut self, id: &str, result: Result<Vec<CatalogEntry>, String>) -> Self {
            self.responses.insert(id.to_string(), result);
            self
        }
    }

    #[async_trait]
    impl CatalogFetcher for FakeFetcher {
        async fn fetch(&self, source: &RegistrySource) -> Result<Vec<CatalogEntry>, String> {
            let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_inflight.fetch_max(now, Ordering::SeqCst);
            if self.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            }
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            self.responses
                .get(&source.id)
                .cloned()
                .unwrap_or_else(|| Err("no canned response".to_string()))
        }
    }

    #[tokio::test]
    async fn collect_catalogs_preserves_source_order() {
        let sources: Vec<RegistrySource> = (1..=5).map(|n| source(&format!("src-{n}"))).collect();
        let enabled: Vec<&RegistrySource> = sources.iter().collect();

        let mut fetcher = FakeFetcher::new(0);
        for n in 1..=5 {
            let id = format!("src-{n}");
            fetcher = fetcher.with(&id, Ok(vec![entry(&format!("evt-{n}"), &id)]));
        }

        let (entries, errors) = collect_catalogs(&fetcher, &enabled).await;
        let ids: Vec<String> = entries.into_iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            vec!["evt-1", "evt-2", "evt-3", "evt-4", "evt-5"],
            "buffered fan-out must preserve per-source input order"
        );
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn collect_catalogs_runs_sources_concurrently() {
        let sources: Vec<RegistrySource> = (1..=4).map(|n| source(&format!("src-{n}"))).collect();
        let enabled: Vec<&RegistrySource> = sources.iter().collect();

        let mut fetcher = FakeFetcher::new(30);
        for n in 1..=4 {
            let id = format!("src-{n}");
            fetcher = fetcher.with(&id, Ok(vec![entry(&format!("evt-{n}"), &id)]));
        }
        let max_inflight = fetcher.max_inflight.clone();

        let (entries, _errors) = collect_catalogs(&fetcher, &enabled).await;
        assert_eq!(entries.len(), 4);
        assert!(
            max_inflight.load(Ordering::SeqCst) > 1,
            "fan-out must overlap fetches (observed max in-flight = {})",
            max_inflight.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn collect_catalogs_isolates_failed_source() {
        let sources = vec![source("src-1"), source("src-2"), source("src-3")];
        let enabled: Vec<&RegistrySource> = sources.iter().collect();

        // Middle source errors — it must drop out without poisoning the
        // surviving two, and surface in the diagnostic `errors` list.
        let fetcher = FakeFetcher::new(0)
            .with("src-1", Ok(vec![entry("evt-1", "src-1")]))
            .with("src-2", Err("boom".to_string()))
            .with("src-3", Ok(vec![entry("evt-3", "src-3")]));

        let (entries, errors) = collect_catalogs(&fetcher, &enabled).await;
        let ids: Vec<String> = entries.into_iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            vec!["evt-1", "evt-3"],
            "a failed source contributes no entries while order is preserved"
        );
        assert_eq!(errors, vec!["src-2: boom".to_string()]);
    }
}
