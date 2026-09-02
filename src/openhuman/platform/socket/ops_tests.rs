use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[tokio::test]
async fn static_token_connection_clears_identity_state_after_disconnect() {
    let manager = SocketManager::new();
    manager
        .connect("http://127.0.0.1:1", "opaque-token")
        .await
        .unwrap();
    let cleared = AtomicBool::new(false);
    connect_static_using(&manager, "http://127.0.0.1:1", "replacement", || {
        assert_eq!(
            manager.get_state().status,
            crate::openhuman::platform::socket::types::ConnectionStatus::Disconnected
        );
        cleared.store(true, Ordering::SeqCst);
    })
    .await
    .unwrap();
    assert!(cleared.load(Ordering::SeqCst));
}
