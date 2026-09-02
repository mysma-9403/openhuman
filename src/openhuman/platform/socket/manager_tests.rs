use super::*;
use serde_json::json;

#[test]
fn new_manager_is_disconnected_with_no_sid() {
    let mgr = SocketManager::new();
    let state = mgr.get_state();
    assert_eq!(state.status, ConnectionStatus::Disconnected);
    assert!(state.socket_id.is_none());
    assert!(state.error.is_none());
    assert!(!mgr.is_connected());
}

#[test]
fn default_impl_matches_new() {
    let a = SocketManager::new();
    let b = SocketManager::default();
    assert_eq!(a.get_state().status, b.get_state().status);
}

#[test]
fn is_connected_tracks_status_transitions() {
    let mgr = SocketManager::new();
    assert!(!mgr.is_connected());
    *mgr.shared.status.write() = ConnectionStatus::Connected;
    assert!(mgr.is_connected());
    *mgr.shared.status.write() = ConnectionStatus::Error;
    assert!(!mgr.is_connected());
}

#[test]
fn get_state_reflects_stored_sid_and_status() {
    let mgr = SocketManager::new();
    *mgr.shared.status.write() = ConnectionStatus::Connected;
    *mgr.shared.socket_id.write() = Some("sid-abc".to_string());
    let state = mgr.get_state();
    assert_eq!(state.status, ConnectionStatus::Connected);
    assert_eq!(state.socket_id.as_deref(), Some("sid-abc"));
}

#[test]
fn get_state_surfaces_stored_error_to_callers() {
    let mgr = SocketManager::new();
    *mgr.shared.error.write() = Some("backend redirected ws→wss; update BACKEND_URL".to_string());
    let state = mgr.get_state();
    assert_eq!(
        state.error.as_deref(),
        Some("backend redirected ws→wss; update BACKEND_URL")
    );
}

#[tokio::test]
async fn emit_without_connection_errors_without_panic() {
    let mgr = SocketManager::new();
    let err = mgr.emit("test.event", json!({"k":"v"})).await.unwrap_err();
    assert_eq!(err, "Not connected");
}

#[tokio::test]
async fn emit_with_ack_without_connection_errors_without_waiting() {
    let mgr = SocketManager::new();
    let err = mgr
        .emit_with_ack("test.event", json!({"k":"v"}), Duration::from_secs(30))
        .await
        .unwrap_err();
    assert_eq!(err, "Not connected");
}

#[tokio::test]
async fn emit_with_ack_uses_emit_queue_while_connecting() {
    let mgr = SocketManager::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    *mgr.emit_tx.lock().await = Some(tx);
    *mgr.shared.status.write() = ConnectionStatus::Connecting;

    let result = mgr
        .emit_with_ack("test.event", json!({"k": "v"}), Duration::from_millis(10))
        .await;

    let queued = rx
        .try_recv()
        .unwrap_or_else(|_| panic!("expected queued ACK emit, got result={result:?}"));
    assert_eq!(queued, r#"421["test.event",{"k":"v"}]"#);
    let err = result.unwrap_err();
    assert!(
        err.starts_with("Socket ack timeout for event test.event ack_id=1"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn disconnect_on_fresh_manager_is_idempotent() {
    let mgr = SocketManager::new();
    assert!(mgr.disconnect().await.is_ok());
    // Calling again must still succeed.
    assert!(mgr.disconnect().await.is_ok());
    assert_eq!(mgr.get_state().status, ConnectionStatus::Disconnected);
}

#[tokio::test]
async fn a_timed_out_socket_loop_is_aborted_and_joined() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MarksDrop(Arc<AtomicBool>);
    impl Drop for MarksDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(AtomicBool::new(false));
    let dropped_in_task = Arc::clone(&dropped);
    let handle = tokio::spawn(async move {
        let _guard = MarksDrop(dropped_in_task);
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;

    terminate_loop(handle, Duration::from_millis(1)).await;

    assert!(
        dropped.load(Ordering::SeqCst),
        "terminate_loop must join the aborted task before returning"
    );
}

#[tokio::test]
async fn identity_rebind_transactions_are_serialized() {
    let manager = Arc::new(SocketManager::new());
    let first = manager.lock_identity_rebind().await;

    let waiting_manager = Arc::clone(&manager);
    let mut waiter = tokio::spawn(async move {
        let _second = waiting_manager.lock_identity_rebind().await;
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut waiter)
            .await
            .is_err(),
        "a second account rebind must not interleave with the first"
    );

    drop(first);
    tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("the next rebind should proceed after the first commits")
        .unwrap();
}

#[test]
fn emit_state_change_is_safe_to_call_on_empty_shared() {
    let shared = SharedState {
        webhook_router: RwLock::new(None),
        ack_registry: AckRegistry::default(),
        status: RwLock::new(ConnectionStatus::Connecting),
        socket_id: RwLock::new(None),
        error: RwLock::new(None),
    };
    // Must not panic even with all default state.
    emit_state_change(&shared);
}

#[test]
fn emit_server_event_is_safe_without_subscribers() {
    let shared = SharedState {
        webhook_router: RwLock::new(None),
        ack_registry: AckRegistry::default(),
        status: RwLock::new(ConnectionStatus::Connected),
        socket_id: RwLock::new(Some("x".into())),
        error: RwLock::new(None),
    };
    // Pure logging — must not touch state or panic.
    emit_server_event(&shared, "any.event", json!({}));
    assert_eq!(*shared.status.read(), ConnectionStatus::Connected);
}

#[test]
fn set_webhook_router_populates_the_shared_slot() {
    let mgr = SocketManager::new();
    assert!(mgr.shared.webhook_router.read().is_none());
    let router = Arc::new(WebhookRouter::new(None));
    mgr.set_webhook_router(router);
    assert!(mgr.shared.webhook_router.read().is_some());
}

#[test]
fn set_webhook_router_overwrites_previous_router() {
    // Replacing the router is allowed so callers can hot-swap during
    // reconfiguration — this test nails that observable behaviour down.
    let mgr = SocketManager::new();
    mgr.set_webhook_router(Arc::new(WebhookRouter::new(None)));
    let second = Arc::new(WebhookRouter::new(None));
    let second_ptr = Arc::as_ptr(&second);
    mgr.set_webhook_router(Arc::clone(&second));
    let stored = mgr.shared.webhook_router.read().clone().unwrap();
    assert!(std::ptr::eq(Arc::as_ptr(&stored), second_ptr));
}

#[tokio::test]
async fn emit_after_disconnect_errors_not_connected() {
    // Even without ever calling connect(), the disconnect() call path
    // leaves the emit channel torn down — and emit() must reject.
    let mgr = SocketManager::new();
    mgr.disconnect().await.unwrap();
    let err = mgr.emit("x", json!({})).await.unwrap_err();
    assert_eq!(err, "Not connected");
}

/// Empty-token guard at the `SocketManager::connect` boundary:
/// the RPC caller must receive an `Err` immediately — not
/// `{"status":"Connecting"}` — so the UI can surface an actionable error.
#[tokio::test]
async fn connect_rejects_empty_token_and_returns_err() {
    let mgr = SocketManager::new();

    // Bare empty string.
    let err = mgr.connect("http://localhost:1", "").await.unwrap_err();
    assert!(
        err.contains("empty session token"),
        "expected 'empty session token' in error, got: {err}"
    );
    assert_eq!(mgr.get_state().status, ConnectionStatus::Disconnected);

    // Whitespace-only string (trim check).
    let err = mgr.connect("http://localhost:1", "   ").await.unwrap_err();
    assert!(err.contains("empty session token"), "{err}");
    assert_eq!(mgr.get_state().status, ConnectionStatus::Disconnected);
}
