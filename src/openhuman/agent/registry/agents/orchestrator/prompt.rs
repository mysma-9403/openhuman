//! System prompt builder for the `orchestrator` built-in agent.
//!
//! The orchestrator follows a direct-first policy: respond directly or use
//! cheap direct tools whenever possible, and delegate only for specialised
//! execution. It never executes Composio actions itself; the integration
//! block points to the single collapsed `delegate_to_integrations_agent`
//! tool (synthesised by `orchestrator_tools::collect_orchestrator_tools`,
//! #1335) for true external-service operations, with the toolkit slug
//! passed as an argument. That prose lives here (not in the shared
//! prompts module) so the skill-executor voice stays in
//! `integrations_agent/prompt.rs` and nobody has to branch on `agent_id`
//! in a shared section impl.

use crate::openhuman::agent::context::prompt::{
    render_datetime, render_identity, render_tools, render_user_files, render_workspace,
    ConnectedIntegration, PromptContext, ToolCallFormat,
};
use crate::openhuman::skills::ops_types::Workflow;
use crate::openhuman::tools::orchestrator_tools::sanitise_slug;
use anyhow::Result;
use std::fmt::Write;

const ARCHETYPE: &str = include_str!("prompt.md");

pub fn build(ctx: &PromptContext<'_>) -> Result<String> {
    let mut out = String::with_capacity(8192);

    // Identity leads the prompt (#5701): SOUL.md is the product persona every
    // opted-in agent shares, ROLE.md is this agent's own role brief. Both are
    // workspace files, so tuning either is an edit rather than a rebuild.
    //
    // Rendered here rather than via `IdentitySection` because the orchestrator
    // is a `PromptSource::Dynamic` agent: `SystemPromptBuilder::from_dynamic`
    // installs only this builder and never consults `omit_identity`, so the
    // section chain that would otherwise inject these files does not run for
    // us. Same reason `render_user_files` is called by hand just below.
    let identity = render_identity(ctx)?;
    if !identity.trim().is_empty() {
        out.push_str(identity.trim_end());
        out.push_str("\n\n");
    }

    out.push_str(ARCHETYPE.trim_end());
    out.push_str("\n\n");

    let user_files = render_user_files(ctx)?;
    if !user_files.trim().is_empty() {
        out.push_str(user_files.trim_end());
        out.push_str("\n\n");
    }

    let identities = ctx.connected_identities_md.as_str();
    if !identities.trim().is_empty() {
        out.push_str(identities.trim_end());
        out.push_str("\n\n");
    }

    let skills = render_installed_skills(ctx.workflows);
    if !skills.trim().is_empty() {
        out.push_str(skills.trim_end());
        out.push_str("\n\n");
    }

    let integrations = render_delegation_guide(ctx.connected_integrations, ctx.tool_call_format);
    if !integrations.trim().is_empty() {
        out.push_str(integrations.trim_end());
        out.push_str("\n\n");
    }

    let mcp_servers = render_connected_mcp_servers();
    if !mcp_servers.trim().is_empty() {
        out.push_str(mcp_servers.trim_end());
        out.push_str("\n\n");
    }

    let tools = render_tools(ctx)?;
    if !tools.trim().is_empty() {
        out.push_str(tools.trim_end());
        out.push_str("\n\n");
    }

    // NOTE: the shared grounding / anti-hallucination contract is appended
    // centrally by `SystemPromptBuilder::build` (and the narrow sub-agent
    // renderer), so every agent inherits it without each `prompt.rs` having
    // to splice it in. Do not render it here, or it will appear twice.

    let datetime = render_datetime(ctx)?;
    if !datetime.trim().is_empty() {
        out.push_str(datetime.trim_end());
        out.push_str("\n\n");
    }

    // The Master Agent can execute coding work directly, so it needs the
    // canonical action-root instructions before it receives the tool list.
    let workspace = render_workspace(ctx)?;
    if !workspace.trim().is_empty() {
        out.push_str(workspace.trim_end());
        out.push_str("\n\n");
    }

    Ok(out)
}

/// Render the `## Installed Skills` section listing locally installed
/// workflows so the orchestrator knows what's available without calling
/// `list_workflows` on every turn. Omitted when no skills are installed.
fn render_installed_skills(skills: &[Workflow]) -> String {
    if skills.is_empty() {
        tracing::debug!("[orchestrator-prompt] no installed skills, section omitted");
        return String::new();
    }
    tracing::debug!(
        count = skills.len(),
        "[orchestrator-prompt] rendering installed skills section"
    );
    let mut out = String::from(
        "## Installed Skills\n\n\
         The following skills are installed locally. Run one with `run_skill` \
         (name the skill and what you want done); it loads and runs the skill in an \
         isolated worker and returns only the result, plus a `## Handoff Plan` for any \
         step the worker couldn't perform — execute those steps yourself under the \
         approval gate. Use `describe_workflow` for full details on one of THESE \
         installed skills (it only knows about entries in this list, not Flows \
         automations — do not call it with a Flows `workflow_id`, it will error). Use \
         `skill_registry_browse` / `skill_registry_search` to find and install new skills. \
         For Flows automations (build/inspect/run a tinyflows workflow), use \
         `build_workflow` / the workflow_builder delegate instead.\n\n",
    );
    for skill in skills {
        let id = if skill.dir_name.is_empty() {
            &skill.name
        } else {
            &skill.dir_name
        };
        let desc = if skill.description.is_empty() {
            "(no description)".to_string()
        } else {
            // Skill descriptions are third-party metadata injected verbatim
            // into the system prompt on EVERY turn. Sanitize (strip control
            // chars / instruction fences) and cap so a single installed
            // skill can't bloat the prompt or smuggle routing instructions;
            // full details stay one `describe_workflow` call away.
            crate::openhuman::util::sanitize::sanitize_for_llm(&skill.description, 240)
                .replace(['\n', '\t'], " ")
                .trim()
                .to_string()
        };
        let _ = writeln!(out, "- **{id}**: {desc}");
    }
    out
}

/// Render the `## Connected MCP Servers` block from the live connection
/// registry. The MCP analogue of [`render_delegation_guide`]: it lists each
/// connected MCP server + the tools it exposes and tells the orchestrator to
/// route matching requests through the single `use_mcp_server` delegate (the
/// `mcp_agent` worker) — NOT to call those tools itself or claim it can't.
/// This is what lets the orchestrator pick up a connected server *without the
/// user naming it* (e.g. a connected "weather" server answering "what's the
/// weather in Tokyo?").
///
/// Reads the global connection map via a guarded `block_on` — the same
/// pattern `tool_registry::ops::registry_entries` uses. `block_in_place`
/// requires the multi-threaded runtime; single-threaded contexts (unit
/// tests) fall back to an empty list and the section is omitted.
fn render_connected_mcp_servers() -> String {
    use crate::openhuman::mcp::registry::connections;
    let servers = match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(connections::connected_overview()))
        }
        _ => Vec::new(),
    };
    format_connected_mcp_block(&servers)
}

/// Pure formatter for the connected-MCP block — split from
/// [`render_connected_mcp_servers`] so it is unit-testable without a live
/// connection registry. Empty input → empty string (section omitted).
fn format_connected_mcp_block(
    servers: &[crate::openhuman::mcp::registry::connections::ConnectedServerOverview],
) -> String {
    if servers.is_empty() {
        return String::new();
    }
    // Keep the block compact — describe each server (the capability signal),
    // not its full toolset. Mirrors the Composio `## Connected Integrations`
    // block (`**Toolkit** (slug): description`). The `mcp_agent` discovers
    // and lists each server's actual tools downstream via
    // `mcp_registry_list_tools`, so the orchestrator only needs to know a
    // server exists and roughly what it does, in order to route.
    let mut out = String::from(
        "## Connected MCP Servers\n\n\
         IMPORTANT: The user has connected the MCP server(s) below. To act on any request \
         a connected server can satisfy, you MUST delegate with `use_mcp_server` — you do \
         NOT have direct access to these servers, and you must never claim you can't do \
         something a connected server clearly can without delegating first. `use_mcp_server` \
         routes to the MCP agent, which discovers the server's tools and calls the right one. \
         Pass a plain-language task; do not pass server ids or tool names yourself.\n\n",
    );
    for s in servers {
        let name = if s.display_name.trim().is_empty() {
            s.qualified_name.as_str()
        } else {
            s.display_name.as_str()
        };
        // The registry/install `description` is UNTRUSTED free-form metadata.
        // It is interpolated into the orchestrator system prompt verbatim, so
        // run it through the same strip-control + strip-instruction-fence +
        // byte-bound pipeline used for remote tool metadata before trusting it
        // (a malicious description could otherwise smuggle routing-overriding
        // instructions into the prompt). Flatten newlines/tabs so a single
        // list item can't be broken or hijacked across lines.
        let desc_raw = s.description.as_deref().unwrap_or("").trim();
        let desc = if desc_raw.is_empty() {
            String::new()
        } else {
            crate::openhuman::util::sanitize::sanitize_for_llm(desc_raw, 240)
                .replace(['\n', '\t'], " ")
                .trim()
                .to_string()
        };
        if !desc.is_empty() {
            let _ = writeln!(out, "- **{name}** (`{}`): {desc}", s.qualified_name);
        } else {
            // No registry description — fall back to a tool-count hint so the
            // line still conveys the server has callable capability.
            let _ = writeln!(
                out,
                "- **{name}** (`{}`) — {} tool{} available",
                s.qualified_name,
                s.tools.len(),
                if s.tools.len() == 1 { "" } else { "s" }
            );
        }
    }
    out
}

/// Render the delegator-voice `## Connected Integrations` block. Only
/// toolkits the user has actively connected are listed — unauthorised
/// toolkits are hidden so the orchestrator cannot hallucinate a delegation
/// to an integration whose `delegate_*` tool does not actually exist.
/// When every toolkit is unconnected the whole section is omitted.
///
/// The tool name printed in the prompt is derived with the same
/// `sanitise_slug` function that `collect_orchestrator_tools` uses when
/// synthesising the real tool objects, so the names in the prompt always
/// match the names in the function-calling schema.
///
/// `tool_call_format` lets the guide adapt to the active provider. Providers
/// with native structured tool-calling (`ToolCallFormat::Native`) get the
/// historic guide unchanged. Text-protocol providers (`PFormat`/`Json`) — the
/// dispatcher chosen for models that force `native_tool_calling = false`, i.e.
/// local runtimes like Ollama / LM Studio / MLX / llama.cpp — additionally get
/// an explicit "when NOT to delegate" carve-out. Weak local models over-select
/// from the prose tool catalogue and the coercive "you MUST delegate" wording,
/// spuriously routing greetings and local-filesystem actions into
/// `delegate_to_integrations_agent` (issue #4361: "Ciao" → Connections,
/// "create a folder on Desktop" → Calendar). The carve-out is additive: the
/// always-delegate contract for genuine service requests is preserved.
fn render_delegation_guide(
    integrations: &[ConnectedIntegration],
    tool_call_format: ToolCallFormat,
) -> String {
    let connected: Vec<&ConnectedIntegration> =
        integrations.iter().filter(|ci| ci.connected).collect();
    tracing::debug!(
        total_integrations = integrations.len(),
        connected_count = connected.len(),
        "[delegation-guide] rendering integration section ({} connected / {} total)",
        connected.len(),
        integrations.len()
    );
    if connected.is_empty() {
        tracing::debug!("[delegation-guide] section omitted — no connected integrations");
        return String::new();
    }
    let mut out = String::from(
        "## Connected Integrations\n\n\
         IMPORTANT: You MUST use the `delegate_to_integrations_agent` tool for any request \
         involving connected services. You do NOT have direct access to these services — all \
         interaction must go through delegation. Delegate here ONLY when the request actually \
         operates on a connected service's data or actions; a connected service is not a reason \
         to touch it for general-knowledge, web/news, headline, date/time, or math questions. \
         Never claim you cannot access a connected \
         service without first attempting delegation.\n\n\
         The following services have an active connection. Their tool implementations \
         live inside the `integrations_agent` sub-agent — NOT in your own tool list. \
         Delegate with `delegate_to_integrations_agent`, passing the toolkit slug as \
         `toolkit`:\n\n",
    );
    for ci in connected {
        // Use the same slug canonicalisation as `collect_orchestrator_tools`
        // so the `toolkit` arg the orchestrator emits always matches the
        // enum the synthesised tool accepts.
        let slug = sanitise_slug(&ci.toolkit);
        if ci.connections.len() > 1 {
            let _ = writeln!(
                out,
                "- **{}** (`toolkit: \"{}\"`, {} accounts connected): {}",
                ci.toolkit,
                slug,
                ci.connections.len(),
                ci.description
            );
            for conn in &ci.connections {
                let label = conn.label.as_deref().unwrap_or("(unlabeled)");
                let default_marker = if conn.is_default { " [default]" } else { "" };
                let _ = writeln!(
                    out,
                    "  - `connection_id: \"{}\"` — {}{}",
                    conn.connection_id, label, default_marker
                );
            }
        } else {
            let _ = writeln!(
                out,
                "- **{}** (`toolkit: \"{}\"`): {}",
                ci.toolkit, slug, ci.description
            );
        }
    }
    // CRITICAL behavioural rule. Without this, the orchestrator answers
    // "can you do X with {toolkit}?" from its training-data priors about
    // "what gmail/notion/slack usually does", which is consistently a
    // SUBSET of the real per-toolkit catalogue (no bulk-delete, no
    // batch-modify, no admin/destructive actions, etc.). The result is a
    // confident wrong refusal ("nope, I can't delete emails") even when
    // the action is in the actual tool list. The `integrations_agent`
    // has the ground-truth tool catalogue (`tools` + `gated_tools`); only
    // it can answer "can I do X?" honestly. Force-delegate capability
    // questions, not just task requests.
    // The cross-chat bullet names the canonical header literal verbatim
    // so the model knows exactly which block to mistrust. Sourced from
    // CROSS_CHAT_HEADER (single source of truth) — drift would silently
    // detune the rule.
    let cross_chat_header_for_prompt =
        crate::openhuman::memory::agent::memory_loader::CROSS_CHAT_HEADER.trim_end();
    let _ = write!(
        out,
        "\n### Capability questions about connected toolkits\n\n\
         Your prior knowledge of \"what a toolkit can do\" is UNRELIABLE — the \
         real per-toolkit catalogue is wider than the common-knowledge summary \
         (e.g. Gmail exposes bulk delete, batch modify, thread trash, etc.) and \
         the user may have enabled scopes that expose further destructive actions. \
         Therefore:\n\n\
         - If the user asks **\"can you do X with {{toolkit}}?\"** or \"does \
         {{toolkit}} support Y?\" for a connected toolkit above, **DO NOT** answer \
         from priors. **DELEGATE** to `integrations_agent` first and let it \
         inspect its live tool list (including `gated_tools` behind permission \
         toggles) before answering.\n\
         - If the user requests an **action** on a connected toolkit (delete, \
         move, send, modify, label, etc.), **DELEGATE immediately**. Do not \
         pre-emptively refuse with \"I can't do that\" — that's a confabulation \
         unless `integrations_agent` itself has already reported the action as \
         unavailable.\n\
         - The only honest \"no\" comes back from a delegation that found the \
         action neither in the visible `tools` list nor in the `gated_tools` \
         (permission-toggle) list of the sub-agent.\n\
         - **Cross-chat context is historical, not authoritative.** If the \
         `{cross_chat_header_for_prompt}` block contains a past \"I can / can't \
         do X with {{toolkit}}\" statement, treat it as a snapshot from an \
         earlier moment. The tool list, connected integrations, and per-toolkit \
         scope toggles (read / write / admin) can all change between chats — a \
         past refusal may be stale. Verify against the **current** `## Connected \
         Integrations` block above and (when in doubt) **DELEGATE** before \
         quoting any past capability claim. Never echo a stale \"I can't\" \
         without re-checking.\n\n",
    );

    // Provider-aware guardrail (#4361). Native-tool-calling providers keep the
    // guide byte-identical. Text-protocol providers (PFormat/Json) — the
    // dispatcher used when a model forces `native_tool_calling = false`, i.e.
    // local runtimes (Ollama / LM Studio / MLX / llama.cpp) — see the whole
    // tool catalogue rendered as prose and are steered by the coercive "you
    // MUST delegate" wording above. Weaker local models then route obviously
    // non-integration requests (greetings, local folder/file actions) into
    // `delegate_to_integrations_agent`, which surfaces "Viewing your
    // Connections" / calendar mis-maps. Carve those cases out explicitly so a
    // small model does not have to infer them from the coercive block alone.
    if tool_call_format != ToolCallFormat::Native {
        out.push_str(
            "### When NOT to delegate\n\n\
             Some requests are NOT integration work — handle them directly and do NOT call \
             `delegate_to_integrations_agent`:\n\
             - **Greetings and small talk** (\"hi\", \"hello\", \"ciao\", \"thanks\", \"how are \
             you?\") — just reply.\n\
             - **Local-machine actions**: creating, reading, writing, moving, or listing files \
             and folders on this computer (e.g. \"create a folder on the Desktop\", \"make a \
             directory\", \"save this to a file\") — use your local filesystem tools. A local \
             folder/file request is NOT a Calendar, Drive, or any connected-service request.\n\n\
             Delegate ONLY when the request clearly names or operates on one of the connected \
             services listed above (its email, calendar, messages, documents, etc.). When a \
             request mixes a local action with a connected service (\"save my latest email to a \
             file on the Desktop\"), do the local part directly and delegate only the \
             service part.\n\n",
        );
    }

    tracing::debug!(
        section_len = out.len(),
        "[delegation-guide] section emitted ({} bytes)",
        out.len()
    );
    out
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
