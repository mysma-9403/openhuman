//! Workload routing and model-call middleware for native TinyAgents models.

use std::sync::LazyLock;

use async_trait::async_trait;
use tinyagents_harness::context::RunContext;
use tinyagents_harness::events::{AgentEvent, GenerationEnrichment, GenerationPricing};
use tinyagents_harness::middleware::{MiddlewareModelOutcome, ModelHandler, ModelMiddleware};
use tinyagents_harness::retry::FallbackPolicy;
use tinyagents_registry::WorkloadRouter;
use tinyinference_llm::model::{CapabilitySet, ModelRequest};

/// The workload routing table (fallback chains and the vision `image_in` gate)
/// projected into the crate [`WorkloadRouter`] from the upstream declarative
/// [`WORKLOAD_ROUTES`](tinyinference_llm::model::workload::WORKLOAD_ROUTES) table,
/// which owns the policy.
static OH_WORKLOAD_ROUTER: LazyLock<WorkloadRouter> =
    LazyLock::new(|| WorkloadRouter::from_workload_routes("hint:vision"));

/// The capability needs a turn imposes on every model call, derived from what is
/// cheaply available at harness-assembly time.
///
/// Today the only reliably-derivable, safe-to-require signal is **vision**: when
/// the turn's effective model is the dedicated `hint:vision` tier the turn was
/// routed there because it carries image input (this is exactly what the
/// `model_vision` selection in `subagent_host/ops/graph.rs` encodes), so we
/// require `image_in` — which keeps the primary vision model selectable while
/// filtering any non-vision fallback pre-dispatch.
///
/// Returns `None` (install no gate) when no requirement is derivable, so the
/// common text turn is unaffected. Signals still to thread (see module note and
/// the migration spec): per-call tool-calling and reasoning needs, BYOK vision
/// (needs `Config` + `model_registry.vision`), and true per-message image
/// presence rather than the tier proxy.
pub(super) fn turn_required_capabilities(model: &str) -> Option<CapabilitySet> {
    OH_WORKLOAD_ROUTER.required_capabilities(model)
}

/// Build the [`FallbackPolicy`] for a turn whose effective/primary model is
/// `model` (issue #4249, Workstream 02.2). The returned chain is `[primary,
/// alternate…]` — the crate's [`FallbackPolicy::next_after`] traversal expects the
/// current (primary) name as the first entry and yields each subsequent alternate.
///
/// The chain now comes straight from the declarative [`OH_WORKLOAD_ROUTER`]
/// (`fallback_policy` leads with the primary, then the tier's same-family
/// alternates). Returns `None` when no same-family alternate exists (vision, or
/// a raw non-tier model string), leaving the turn primary-only.
pub(super) fn route_fallback_policy(model: &str) -> Option<FallbackPolicy> {
    let policy = OH_WORKLOAD_ROUTER.fallback_policy(model);
    match &policy {
        Some(p) => tracing::debug!(
            route = model,
            chain = ?p.models,
            "[fallback] configured SDK-owned cross-route fallback chain"
        ),
        None => tracing::debug!(
            route = model,
            "[fallback] no same-family fallback route; turn is primary-only"
        ),
    }
    policy
}

/// Captures the concrete provider/model/host-route selected for a successful
/// call from TinyInference's canonical response metadata.
///
/// `tinyinference_llm::model::RouteRecordingModel` decorates both unary
/// responses and stream terminal metadata. The harness folds a stream into its
/// terminal `ModelResponse` before middleware regains control, so this one
/// typed boundary records primary and fallback routes identically without an
/// OpenHuman model wrapper or task-local propagation.
pub(super) struct ResolvedRouteMiddleware {
    /// Explicit configured fallback when a provider cannot return route metadata.
    provider_id: String,
    model: String,
}

impl ResolvedRouteMiddleware {
    pub(super) fn new(provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model: model.into(),
        }
    }
}

#[async_trait]
impl ModelMiddleware<(), crate::agent::tinyagents::host::OpenHumanRunContext>
    for ResolvedRouteMiddleware
{
    fn name(&self) -> &str {
        "openhuman.resolved_route"
    }

    async fn wrap_model(
        &self,
        ctx: &mut RunContext<crate::agent::tinyagents::host::OpenHumanRunContext>,
        state: &(),
        request: ModelRequest,
        next: ModelHandler<'_, (), crate::agent::tinyagents::host::OpenHumanRunContext>,
    ) -> tinyagents_harness::Result<MiddlewareModelOutcome> {
        let call_id = request
            .correlation
            .as_ref()
            .map(|correlation| correlation.call_id.clone())
            .or_else(|| {
                ctx.active_model_call
                    .as_ref()
                    .map(|call_id| call_id.as_str().to_owned())
            });
        if let Some(call_id) = &call_id {
            let provider_id =
                valid_trace_provider_id(&self.provider_id).then(|| self.provider_id.clone());
            if let Some(provider_id) = provider_id {
                ctx.events.emit(AgentEvent::GenerationEnriched {
                    call_id: call_id.clone().into(),
                    enrichment: GenerationEnrichment {
                        provider_id: Some(provider_id),
                        ..Default::default()
                    },
                });
            }
        }
        let outcome = next.run(ctx, state, request).await?;
        let response = outcome.into_response();
        if let Some(route) = response.resolved_route.clone() {
            *ctx.data
                .resolved_route
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(route);
        }
        if let Some(call_id) = call_id {
            let route = response.resolved_route.as_ref();
            let model = route
                .map(|route| route.model.as_str())
                .or_else(|| {
                    response
                        .resolved_model
                        .as_ref()
                        .map(|resolved| resolved.name.as_str())
                })
                .unwrap_or(&self.model);
            let provider_id = route
                .map(|route| route.provider.as_str())
                .unwrap_or(&self.provider_id);
            let provider_id = valid_trace_provider_id(provider_id).then(|| provider_id.to_owned());
            let pricing = if model.trim().to_ascii_lowercase().ends_with(":free") {
                Some(GenerationPricing {
                    input_per_mtok_usd: Some(0.0),
                    cached_input_per_mtok_usd: Some(0.0),
                    output_per_mtok_usd: Some(0.0),
                })
            } else {
                tinyagents_registry::pricing::lookup(model).map(|price| GenerationPricing {
                    input_per_mtok_usd: Some(price.input_per_mtok_usd),
                    cached_input_per_mtok_usd: Some(price.cached_input_per_mtok_usd),
                    output_per_mtok_usd: Some(price.output_per_mtok_usd),
                })
            };
            let billed = crate::inference::provider::model::usage_info_from_response(&response);
            let estimated_cost_usd = if response.served_from_cache
                || billed.is_some_and(|usage| usage.charge_reported && !usage.cost_is_estimate)
            {
                None
            } else {
                billed.and_then(|usage| crate::agent::cost::estimate_call_cost_usd(model, &usage))
            };
            let enrichment = GenerationEnrichment {
                provider_id,
                pricing,
                estimated_cost_usd,
            };
            if enrichment.provider_id.is_some()
                || enrichment.pricing.is_some()
                || enrichment.estimated_cost_usd.is_some()
            {
                ctx.events.emit(AgentEvent::GenerationEnriched {
                    call_id: call_id.into(),
                    enrichment,
                });
            }
        }
        Ok(MiddlewareModelOutcome::from(response))
    }
}

fn valid_trace_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':' | '-')
        })
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
