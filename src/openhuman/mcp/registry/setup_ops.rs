//! RPC handlers for the guided setup flow (`mcp_setup`).
//!
//! Every function keeps the signature and the JSON shape it had before the
//! client moved to `tinymcp`, and delegates to the service [`super::super::host`]
//! holds.
//!
//! # The secret handles have not changed shape
//!
//! The flow is still: mint an opaque `secret://…` handle, prompt the user out
//! of band, and resolve the handle inside the operation that needs the value.
//! The raw value never crosses the model-facing surface. `tinymcp` owns the
//! vault; this layer keeps the part that is this application's — publishing the
//! event that makes the prompt appear, and waiting for the answer.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;

use tinymcp::SecretRef;

use super::helpers::{encode, inject_required_env_keys, require, resolve};

/// Reads a map of credential names to handles.
fn parse_handles(raw: HashMap<String, String>) -> Result<HashMap<String, SecretRef>, String> {
    raw.into_iter()
        .map(|(name, handle)| {
            SecretRef::parse(&handle)
                .map(|parsed| (name, parsed))
                .ok_or_else(|| format!("invalid ref_id `{handle}`"))
        })
        .collect()
}

/// Renders a detail record with the credential names an install would need.
fn detail_payload(
    detail: &tinymcp_bus::RegistryServerDetail,
    required_env_keys: &[String],
) -> Result<Value, String> {
    let mut value = encode(detail)?;
    inject_required_env_keys(&mut value, required_env_keys);
    Ok(value)
}

// ── search ───────────────────────────────────────────────────────────────────

pub async fn mcp_setup_search(
    config: &Config,
    query: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<RpcOutcome<Value>, String> {
    let page = page.unwrap_or(1);
    let page_size = page_size.unwrap_or(20);

    let found = resolve(config)?
        .dynamic()
        .registry_search(query.as_deref(), page, page_size)
        .await
        .map_err(|error| error.to_string())?;

    let count = found.servers.len();
    Ok(RpcOutcome::new(
        json!({
            "servers": found.servers,
            "page": found.page,
            "total_pages": found.total_pages,
        }),
        vec![format!("setup_search returned {count} servers")],
    ))
}

// ── get ──────────────────────────────────────────────────────────────────────

pub async fn mcp_setup_get(
    config: &Config,
    qualified_name: String,
) -> Result<RpcOutcome<Value>, String> {
    let qualified_name = require(&qualified_name, "qualified_name")?;

    let (detail, required_env_keys) = resolve(config)?
        .dynamic()
        .registry_get(&qualified_name)
        .await
        .map_err(|error| error.to_string())?;

    Ok(RpcOutcome::new(
        json!({ "server": detail_payload(&detail, &required_env_keys)? }),
        vec![format!("setup_get ok qualified_name={qualified_name}")],
    ))
}

// ── request_secret ───────────────────────────────────────────────────────────

/// Mints a handle, asks the user for the value, and waits for it.
///
/// The wait is here rather than in `tinymcp` because the prompt is: this layer
/// publishes the event a user interface renders, so it is the layer that knows
/// when an answer can arrive.
pub async fn mcp_setup_request_secret(
    config: &Config,
    key_name: String,
    prompt: String,
) -> Result<RpcOutcome<Value>, String> {
    let key_name = require(&key_name, "key_name")?;
    let prompt = require(&prompt, "prompt")?;

    let service = resolve(config)?;
    let vault = service.dynamic().vault();
    let (handle, receiver) = vault.request(&key_name).await;

    BUS.publish(DomainEvent::McpSetupSecretRequested {
        ref_id: handle.as_str().to_string(),
        key_name: key_name.clone(),
        prompt,
    });
    tracing::info!(
        handle = handle.as_str(),
        key_name,
        "[mcp-setup] awaiting a secret from the user"
    );

    vault
        .await_fulfillment(&handle, receiver)
        .await
        .map_err(|error| error.to_string())?;

    tracing::info!(handle = handle.as_str(), "[mcp-setup] the secret arrived");

    Ok(RpcOutcome::new(
        json!({ "ref": handle.as_str(), "key_name": key_name }),
        vec![format!("collected secret for key={key_name}")],
    ))
}

// ── submit_secret ────────────────────────────────────────────────────────────

pub async fn mcp_setup_submit_secret(
    config: &Config,
    ref_id: String,
    value: String,
) -> Result<RpcOutcome<Value>, String> {
    let handle = SecretRef::parse(&ref_id).ok_or_else(|| format!("invalid ref_id `{ref_id}`"))?;

    let accepted = resolve(config)?
        .dynamic()
        .vault()
        .submit(&handle, value)
        .await;

    if !accepted {
        return Err(format!(
            "ref {} unknown or already submitted",
            handle.as_str()
        ));
    }

    Ok(RpcOutcome::new(
        json!({ "ref": handle.as_str(), "fulfilled": true }),
        vec![format!("submitted secret for ref={}", handle.as_str())],
    ))
}

// ── test_connection ──────────────────────────────────────────────────────────

/// Dials a server with the collected credentials without installing it.
///
/// A dial that fails is reported as `ok: false` with the reason rather than as
/// an error: the operation asked for — finding out whether it works — succeeded,
/// and the agent needs the reason to tell the user what to fix.
pub async fn mcp_setup_test_connection(
    config: &Config,
    qualified_name: String,
    env_refs: HashMap<String, String>,
) -> Result<RpcOutcome<Value>, String> {
    let qualified_name = require(&qualified_name, "qualified_name")?;
    let handles = parse_handles(env_refs)?;

    match resolve(config)?
        .dynamic()
        .setup_test_connection(&qualified_name, &handles)
        .await
    {
        Ok(tools) => {
            let tools = super::tools_safe_for_agent(&qualified_name, tools);
            let count = tools.len();
            Ok(RpcOutcome::new(
                json!({ "ok": true, "tools": tools }),
                vec![format!(
                    "test_connection ok for {qualified_name}: {count} tools"
                )],
            ))
        }
        Err(error) => Ok(RpcOutcome::new(
            json!({ "ok": false, "error": error.to_string() }),
            vec![format!(
                "test_connection failed for {qualified_name}: {error}"
            )],
        )),
    }
}

// ── install_and_connect ──────────────────────────────────────────────────────

pub async fn mcp_setup_install_and_connect(
    config: &Config,
    qualified_name: String,
    env_refs: HashMap<String, String>,
) -> Result<RpcOutcome<Value>, String> {
    let qualified_name = require(&qualified_name, "qualified_name")?;
    let handles = parse_handles(env_refs)?;

    let outcome = resolve(config)?
        .dynamic()
        .setup_install_and_connect(&qualified_name, &handles, None)
        .await
        .map_err(|error| error.to_string())?;

    let tools = super::tools_safe_for_agent(&outcome.server_id, outcome.tools);
    let tool_count = u32::try_from(tools.len()).unwrap_or(u32::MAX);

    BUS.publish(DomainEvent::McpServerInstalled {
        server_id: outcome.server_id.clone(),
        qualified_name: qualified_name.clone(),
    });
    // Unconditional, like the `connect` handler's. The call returning `Ok` is
    // what says the connection succeeded; a server exposing only resources or
    // prompts connects with zero tools, and gating on the count would leave
    // anything tracking connection state believing it never came up.
    BUS.publish(DomainEvent::McpServerConnected {
        server_id: outcome.server_id.clone(),
        tool_count,
    });

    Ok(RpcOutcome::new(
        json!({
            "server_id": outcome.server_id,
            "qualified_name": qualified_name,
            "tools": tools,
        }),
        vec![format!(
            "installed and connected server_id={} tools={tool_count}",
            outcome.server_id
        )],
    ))
}

#[cfg(test)]
#[path = "setup_ops_tests.rs"]
mod tests;
