//! SocketManager — persistent Rust-native Socket.IO connection via WebSocket.
//!
//! Implements Engine.IO v4 and Socket.IO v4 protocols directly over WebSocket
//! using `tokio-tungstenite` with `rustls` TLS.
//!
//! Responsibilities:
//! - MCP `listTools` / `toolCall` handled directly via the WorkflowRegistry
//! - Non-MCP server events forwarded to running skills and to the frontend
//! - Connection state logging for observability
//! - Automatic reconnection with exponential backoff

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};

use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Duration;

use crate::api::models::socket::{ConnectionStatus, SocketState};
use crate::openhuman::skills::webhooks::WebhookRouter;

use super::token_provider::{static_token_provider, TokenProvider};
use super::ws_loop::ws_loop;

// ---------------------------------------------------------------------------
// Global accessor
// ---------------------------------------------------------------------------

static GLOBAL_SOCKET_MANAGER: OnceLock<Arc<SocketManager>> = OnceLock::new();

/// Register the global `SocketManager` instance (called once during bootstrap).
pub fn set_global_socket_manager(mgr: Arc<SocketManager>) {
    if GLOBAL_SOCKET_MANAGER.set(mgr).is_err() {
        log::warn!("[socket] global SocketManager already set — ignoring duplicate");
    }
}

/// Retrieve the global `SocketManager`, if initialized.
pub fn global_socket_manager() -> Option<&'static Arc<SocketManager>> {
    GLOBAL_SOCKET_MANAGER.get()
}

// ---------------------------------------------------------------------------
// Shared state (visible to sibling modules)
// ---------------------------------------------------------------------------

/// State shared between the `SocketManager` handle and the background loop.
pub(super) struct SharedState {
    /// Router for delivering incoming webhooks to skills.
    pub(super) webhook_router: RwLock<Option<Arc<WebhookRouter>>>,
    /// Pending Socket.IO ACK callbacks keyed by outbound ack id.
    pub(super) ack_registry: AckRegistry,
    /// Current connection status.
    pub(super) status: RwLock<ConnectionStatus>,
    /// Socket ID assigned by the server.
    pub(super) socket_id: RwLock<Option<String>>,
    /// Last user-visible connection warning surfaced through `SocketState.error`
    /// (e.g. "backend redirected ws→wss; update BACKEND_URL"). Cleared on every
    /// successful handshake and on disconnect.
    pub(super) error: RwLock<Option<String>>,
}

pub(super) struct AckRegistry {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>,
}

impl Default for AckRegistry {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl AckRegistry {
    pub(super) fn register(&self) -> (u64, oneshot::Receiver<serde_json::Value>) {
        let ack_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(ack_id, tx);
        (ack_id, rx)
    }

    pub(super) fn resolve(&self, ack_id: u64, data: serde_json::Value) -> bool {
        if let Some(tx) = self.pending.lock().remove(&ack_id) {
            let _ = tx.send(data);
            true
        } else {
            false
        }
    }

    pub(super) fn remove(&self, ack_id: u64) {
        self.pending.lock().remove(&ack_id);
    }

    pub(super) fn cancel_all(&self) {
        self.pending.lock().clear();
    }
}

// ---------------------------------------------------------------------------
// SocketManager
// ---------------------------------------------------------------------------

/// Manages a persistent Socket.IO connection to the backend.
///
/// Handles protocol-level handshakes (Engine.IO / Socket.IO), heartbeats, and
/// automatic reconnection while providing a high-level API for emitting events
/// and syncing tool state.
pub struct SocketManager {
    /// Shared state accessible from both the manager and the background loop.
    pub(super) shared: Arc<SharedState>,
    /// Channel for sending outgoing messages to the background loop.
    emit_tx: tokio::sync::Mutex<Option<mpsc::UnboundedSender<String>>>,
    /// Channel for signaling the background loop to shut down.
    shutdown_tx: tokio::sync::Mutex<Option<watch::Sender<bool>>>,
    /// Join handle for the background connection loop.
    loop_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Serializes identity-sensitive disconnect → bridge bind → connect
    /// transactions while still allowing ordinary emits and state reads.
    identity_rebind: tokio::sync::Mutex<()>,
}

impl SocketManager {
    /// Create a new, disconnected SocketManager.
    pub fn new() -> Self {
        log::debug!("[socket] SocketManager created (disconnected)");
        Self {
            shared: Arc::new(SharedState {
                webhook_router: RwLock::new(None),
                ack_registry: AckRegistry::default(),
                status: RwLock::new(ConnectionStatus::Disconnected),
                socket_id: RwLock::new(None),
                error: RwLock::new(None),
            }),
            emit_tx: tokio::sync::Mutex::new(None),
            shutdown_tx: tokio::sync::Mutex::new(None),
            loop_handle: tokio::sync::Mutex::new(None),
            identity_rebind: tokio::sync::Mutex::new(()),
        }
    }

    /// Lock an identity-sensitive socket rebind for its complete transaction.
    pub(crate) async fn lock_identity_rebind(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.identity_rebind.lock().await
    }

    /// Set the webhook router for skill-targeted webhook delivery.
    pub fn set_webhook_router(&self, router: Arc<WebhookRouter>) {
        log::debug!("[socket] WebhookRouter attached");
        *self.shared.webhook_router.write() = Some(router);
    }

    /// Get the webhook router, if one has been set.
    pub fn webhook_router(&self) -> Option<Arc<WebhookRouter>> {
        self.shared.webhook_router.read().clone()
    }

    /// Get the current socket state (status, ID, error).
    pub fn get_state(&self) -> SocketState {
        SocketState {
            status: *self.shared.status.read(),
            socket_id: self.shared.socket_id.read().clone(),
            error: self.shared.error.read().clone(),
        }
    }

    /// Check if the socket is currently connected.
    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        *self.shared.status.read() == ConnectionStatus::Connected
    }

    // -----------------------------------------------------------------------
    // Connection lifecycle
    // -----------------------------------------------------------------------

    /// Connect to the specified URL using the provided authentication token.
    ///
    /// Spawns a background `ws_loop` that manages the connection with automatic
    /// reconnection and exponential backoff.
    ///
    /// Returns `Err` immediately if `token` is empty — every reconnect attempt
    /// would either 401 at the SIO CONNECT step or fail upstream at the gateway,
    /// producing exactly the kind of retry-storm noise this module is designed to
    /// suppress. Callers receive an actionable error and the RPC response reflects
    /// the actual outcome rather than optimistically reporting `{"status":"Connecting"}`.
    pub async fn connect(&self, url: &str, token: &str) -> Result<(), String> {
        if token.trim().is_empty() {
            log::error!("[socket] connect: refusing to start — empty session token");
            return Err("empty session token — authenticate first".to_string());
        }
        // Wrap the static token in a provider closure. Existing callers that
        // pass a concrete token value continue to work unchanged; the provider
        // returns that same token on every call (static semantics). For
        // live-session refresh, callers should use `connect_with_session` which
        // builds a provider via `token_provider_from_config`.
        let provider = static_token_provider(token.to_string());
        self.spawn_loop(url, provider).await
    }

    /// Connect using a **live-refresh token provider**.
    ///
    /// Unlike [`connect`] which wraps a single static token, this method
    /// accepts a [`TokenProvider`] closure that is called before every
    /// reconnect attempt. Use this when the token may change between retries
    /// (e.g. after a session refresh or re-login) so the loop always sends the
    /// freshest available credential.
    ///
    /// The provider is called immediately to validate that a token is available
    /// before the background task is spawned — callers receive an actionable
    /// `Err` if no token is stored rather than spawning a doomed retry loop.
    pub async fn connect_with_provider(
        &self,
        url: &str,
        token_provider: TokenProvider,
    ) -> Result<(), String> {
        // Validate that a token is available right now before spawning. This
        // mirrors the empty-token guard in `connect()` and ensures callers
        // see an immediate error if the session store is empty.
        match token_provider() {
            Ok(t) if !t.trim().is_empty() => {}
            Ok(_) => {
                log::error!(
                    "[socket] connect_with_provider: refusing to start — provider returned empty token"
                );
                return Err("empty session token — authenticate first".to_string());
            }
            Err(e) => {
                log::error!(
                    "[socket] connect_with_provider: refusing to start — provider error: {e}"
                );
                return Err(e);
            }
        }
        self.spawn_loop(url, token_provider).await
    }

    /// Shared spawn path used by both [`connect`] and [`connect_with_provider`].
    ///
    /// Installs the rustls crypto provider, tears down any existing connection,
    /// constructs the channel pair, and spawns the background `ws_loop` task.
    /// Entry-point-specific validation (empty-token guard, provider pre-check)
    /// is done by the callers before this is called.
    async fn spawn_loop(&self, url: &str, provider: TokenProvider) -> Result<(), String> {
        // Ensure the rustls crypto provider is installed (needed for wss:// TLS).
        // This is a no-op if already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();

        self.disconnect().await?;

        log::info!("[socket] Connecting to {}", url);

        *self.shared.status.write() = ConnectionStatus::Connecting;
        *self.shared.error.write() = None;
        emit_state_change(&self.shared);

        let (emit_tx, emit_rx) = mpsc::unbounded_channel::<String>();
        let internal_tx = emit_tx.clone();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        *self.emit_tx.lock().await = Some(emit_tx);
        *self.shutdown_tx.lock().await = Some(shutdown_tx);

        let url = url.to_string();
        let shared = Arc::clone(&self.shared);

        let handle = tokio::spawn(async move {
            ws_loop(url, provider, shared, emit_rx, shutdown_rx, internal_tx).await;
        });

        *self.loop_handle.lock().await = Some(handle);
        Ok(())
    }

    /// Disconnect from the server and shut down the background loop.
    pub async fn disconnect(&self) -> Result<(), String> {
        super::medulla::workflows::end_connection_generation();
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(true);
        }
        self.shared.ack_registry.cancel_all();
        self.emit_tx.lock().await.take();
        if let Some(handle) = self.loop_handle.lock().await.take() {
            terminate_loop(handle, Duration::from_secs(5)).await;
        }
        *self.shared.status.write() = ConnectionStatus::Disconnected;
        *self.shared.socket_id.write() = None;
        *self.shared.error.write() = None;
        emit_state_change(&self.shared);
        log::debug!("[socket] Disconnected");
        Ok(())
    }

    /// Emit a Socket.IO event to the server.
    pub async fn emit(&self, event: &str, data: serde_json::Value) -> Result<(), String> {
        if let Some(ref tx) = *self.emit_tx.lock().await {
            let msg = encode_sio_event(event, data, None)?;
            tx.send(msg).map_err(|_| "Socket not connected".to_string())
        } else {
            Err("Not connected".to_string())
        }
    }

    /// Emit a Socket.IO event and wait for the backend ACK callback.
    pub async fn emit_with_ack(
        &self,
        event: &str,
        data: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let tx = self
            .emit_tx
            .lock()
            .await
            .clone()
            .ok_or_else(|| "Not connected".to_string())?;
        let (ack_id, ack_rx) = self.shared.ack_registry.register();
        let msg = encode_sio_event(event, data, Some(ack_id))?;
        if let Err(e) = tx.send(msg) {
            self.shared.ack_registry.remove(ack_id);
            return Err(format!("Socket not connected: {e}"));
        }

        log::debug!("[socket] emit_with_ack sent event={event} ack_id={ack_id}");
        match tokio::time::timeout(timeout, ack_rx).await {
            Ok(Ok(data)) => {
                log::debug!("[socket] emit_with_ack resolved event={event} ack_id={ack_id}");
                Ok(data)
            }
            Ok(Err(_)) => Err(format!(
                "Socket ack channel dropped for event {event} ack_id={ack_id}"
            )),
            Err(_) => {
                self.shared.ack_registry.remove(ack_id);
                Err(format!(
                    "Socket ack timeout for event {event} ack_id={ack_id}"
                ))
            }
        }
    }
}

/// Wait for the socket loop to observe shutdown, then abort and join it if a
/// transport operation outlives the grace period.
///
/// Dropping a timed-out `JoinHandle` detaches its task. During an account
/// switch that would let the old credential finish authenticating after the
/// new user's workflow bridge is installed, so timeout must mean termination,
/// not detachment.
async fn terminate_loop(mut handle: tokio::task::JoinHandle<()>, grace: Duration) {
    if tokio::time::timeout(grace, &mut handle).await.is_err() {
        log::warn!("[socket] connection loop did not stop within {grace:?} — aborting");
        handle.abort();
        let _ = handle.await;
    }
}

fn encode_sio_event(
    event: &str,
    data: serde_json::Value,
    ack_id: Option<u64>,
) -> Result<String, String> {
    let payload = serde_json::to_string(&json!([event, data])).map_err(|e| format!("{e}"))?;
    let ack = ack_id.map(|id| id.to_string()).unwrap_or_default();
    Ok(format!("42{ack}{payload}"))
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// State-change helpers (used by sibling modules)
// ---------------------------------------------------------------------------

/// Log a state change for observability.
pub(super) fn emit_state_change(shared: &SharedState) {
    let status = *shared.status.read();
    let socket_id = shared.socket_id.read().clone();
    log::debug!("[socket] State changed: {:?}, sid={:?}", status, socket_id);
}

/// Log a server event for observability.
pub(super) fn emit_server_event(_shared: &SharedState, event_name: &str, _data: serde_json::Value) {
    log::debug!("[socket] Server event: {}", event_name);
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
