use crate::api::models::socket::SocketState;

use super::SocketManager;

async fn connect_static_using(
    manager: &SocketManager,
    url: &str,
    token: &str,
    clear_workflows: impl FnOnce(),
) -> Result<SocketState, String> {
    let _rebind = manager.lock_identity_rebind().await;
    manager.disconnect().await?;
    clear_workflows();
    manager.connect(url, token).await?;
    Ok(manager.get_state())
}

pub async fn connect_static(
    manager: &SocketManager,
    url: &str,
    token: &str,
) -> Result<SocketState, String> {
    log::info!("[socket:rpc] connect — disabling identity-bound workflow plane");
    connect_static_using(
        manager,
        url,
        token,
        super::medulla::workflows::clear_workflow_bridge,
    )
    .await
}

pub async fn disconnect(manager: &SocketManager) -> Result<SocketState, String> {
    log::info!("[socket:rpc] disconnect");
    let _rebind = manager.lock_identity_rebind().await;
    manager.disconnect().await?;
    Ok(manager.get_state())
}

pub async fn connect_with_session(manager: &SocketManager) -> Result<SocketState, String> {
    log::info!("[socket:rpc] connect_with_session — resolving credentials");
    let config =
        std::sync::Arc::new(crate::openhuman::config::rpc::load_config_with_timeout().await?);
    let api_url = crate::api::config::effective_backend_api_url(&config.api_url);
    crate::api::jwt::get_session_token(&config)
        .map_err(|e| format!("failed to read session token: {e}"))?
        .ok_or("no session token stored — user must log in first")?;

    let _rebind = manager.lock_identity_rebind().await;
    manager.disconnect().await?;
    #[cfg(feature = "flows")]
    if crate::core::runtime::context::CoreContext::current()
        .is_some_and(|context| context.domains().flows)
    {
        crate::openhuman::flows::medulla_bridge::install(std::sync::Arc::clone(&config));
    }
    let provider = super::token_provider::token_provider_from_config(config);
    manager.connect_with_provider(&api_url, provider).await?;
    Ok(manager.get_state())
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
