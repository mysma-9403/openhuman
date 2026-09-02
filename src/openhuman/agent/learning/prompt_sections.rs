//! Prompt sections that inject learned context into the agent's system prompt.
//!
//! These sections read pre-fetched data from `PromptContext.learned` — no async
//! or blocking I/O happens during prompt building.
//!
//! ## Phase 3 addition (#566)
//!
//! [`load_learned_from_cache`] reads Active facets from the `FacetCache`
//! (backed by `user_profile_facets`) and returns them as a list of formatted
//! strings suitable for injection into `LearnedContextData.user_profile`.
//!
//! The existing KV-namespace reads in `fetch_learned_context` are preserved
//! (both paths active in this phase; KV path will be removed in a follow-up).
//!
//! ## Phase 4 addition (#566)
//!
//! [`MemoryAccessSection`] — a static prompt section that instructs the agent to
//! call `memory_recall` / `memory_search` before answering questions that draw on
//! prior sessions. Registered after `LearnedContextSection` in the section chain.

use crate::openhuman::agent::context::prompt::{PromptContext, PromptSection};
use anyhow::Result;

/// Injects recent observations and patterns from the learning subsystem.
pub struct LearnedContextSection;

impl LearnedContextSection {
    pub fn new(_memory: std::sync::Arc<dyn crate::openhuman::memory::Memory>) -> Self {
        // Memory parameter kept for API compatibility but data comes from PromptContext.learned
        Self
    }
}

impl PromptSection for LearnedContextSection {
    fn name(&self) -> &str {
        "learned_context"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        if ctx.learned.observations.is_empty() && ctx.learned.patterns.is_empty() {
            return Ok(String::new());
        }

        let mut out = String::from("## Learned Context\n\n");

        if !ctx.learned.observations.is_empty() {
            out.push_str("### Recent Observations\n");
            for obs in &ctx.learned.observations {
                out.push_str("- ");
                out.push_str(obs);
                out.push('\n');
            }
            out.push('\n');
        }

        if !ctx.learned.patterns.is_empty() {
            out.push_str("### Recognized Patterns\n");
            for pat in &ctx.learned.patterns {
                out.push_str("- ");
                out.push_str(pat);
                out.push('\n');
            }
            out.push('\n');
        }

        Ok(out)
    }
}

/// Injects the learned user profile into the system prompt.
pub struct UserProfileSection;

impl UserProfileSection {
    pub fn new(_memory: std::sync::Arc<dyn crate::openhuman::memory::Memory>) -> Self {
        Self
    }
}

impl PromptSection for UserProfileSection {
    fn name(&self) -> &str {
        "user_profile"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        if ctx.learned.user_profile.is_empty() {
            return Ok(String::new());
        }

        let mut out = String::from("## Your standing preferences\n\n");
        for entry in &ctx.learned.user_profile {
            out.push_str("- ");
            out.push_str(entry);
            out.push('\n');
        }
        out.push('\n');
        Ok(out)
    }
}

// ── MemoryAccessSection ───────────────────────────────────────────────────────

/// Static bias instruction that tells the agent to call `memory_recall` before
/// answering questions involving named people, projects, prior decisions, or
/// anything the user mentioned in past sessions.
///
/// The text is frozen at compile time — no I/O at build time.
/// Register this section after [`LearnedContextSection`] in the prompt-section
/// composition order (see `SystemPromptBuilder::with_defaults`).
pub struct MemoryAccessSection;

/// The static prose injected into every system prompt. Kept at ≤ 80 tokens.
pub const MEMORY_ACCESS_INSTRUCTION: &str = "\
## Memory access\n\
\n\
Before answering questions involving named people, projects, threads, prior \
decisions, recurring topics, or anything the user has mentioned in past sessions, \
call `memory_recall` (or `memory_search` for keyword lookups) to retrieve \
relevant context. Surface what matters in your reply; don't stitch together \
continuity from prompt history alone. Skip retrieval for purely procedural \
requests where prior context isn't relevant.";

impl PromptSection for MemoryAccessSection {
    fn name(&self) -> &str {
        "memory_access"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
        Ok(MEMORY_ACCESS_INSTRUCTION.to_string())
    }
}

// ── Cache-backed loader ───────────────────────────────────────────────────────

/// Maximum number of facets to include in the ambient prompt injection.
///
/// Corresponds to "~25 entries total" from the Phase 3 spec.
const CACHE_PROMPT_CAP: usize = 25;

/// Load Active facets from the `FacetCache` and format them for prompt injection.
///
/// Returns a list of strings in the form `class/key: value`, sorted by stability
/// descending within each class, then alphabetically by class. The total is capped
/// at [`CACHE_PROMPT_CAP`] entries.
///
/// Async because the facet store moved behind the memory driver: this used to
/// be a synchronous SQLite read, and is now a driver call. The caller should
/// keep both this path and the existing KV-namespace path active until the KV
/// path is removed in a follow-up phase.
pub async fn load_learned_from_cache(
    cache: &crate::openhuman::agent::learning::cache::FacetCache,
) -> Vec<String> {
    let facets = match cache.list_active().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("[learning::prompt] load_learned_from_cache failed: {e}");
            return Vec::new();
        }
    };

    if facets.is_empty() {
        return Vec::new();
    }

    // Group by class prefix (portion before the first '/'), then sort within
    // each class by stability descending, then by key alphabetically.
    use std::collections::BTreeMap;
    use tinymemory_api::provider::ProfileFacet;
    let mut by_class: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for (idx, f) in facets.iter().enumerate() {
        let class = f
            .key
            .split_once('/')
            .map(|(prefix, _rest)| prefix.to_string())
            .unwrap_or_else(|| "other".to_string());
        by_class.entry(class).or_default().push(idx);
    }

    for indices in by_class.values_mut() {
        indices.sort_by(|&a, &b| {
            facets[b]
                .stability
                .partial_cmp(&facets[a].stability)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| facets[a].key.cmp(&facets[b].key))
        });
    }

    let mut result = Vec::with_capacity(CACHE_PROMPT_CAP);
    'outer: for indices in by_class.values() {
        for &idx in indices {
            if result.len() >= CACHE_PROMPT_CAP {
                break 'outer;
            }
            let f: &ProfileFacet = &facets[idx];
            // Phase 4: render in structured `class/key: value` form so the
            // agent can parse the source. Goal class keeps value-only (full
            // sentence, no key prefix). Pinned entries get a trailing suffix.
            let pinned = if f.user_state == tinymemory_api::provider::UserState::Pinned {
                " *(pinned)*"
            } else {
                ""
            };
            let entry = if f.key.starts_with("goal/") {
                // Goal class: render just the value, it's a sentence.
                format!("{}{}", f.value, pinned)
            } else {
                format!("**{}**: {}{}", f.key, f.value, pinned)
            };
            result.push(entry);
        }
    }

    result
}

#[cfg(test)]
#[path = "prompt_sections_tests.rs"]
mod prompt_sections_tests;

#[cfg(test)]
#[path = "prompt_sections_tests_2_tests.rs"]
mod tests;
