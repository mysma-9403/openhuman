use super::*;
use serde_json::json;

#[test]
fn tunnel_register_response_accepts_backend_ack_shape_without_session_token() {
    let response: TunnelRegisterResponse = serde_json::from_value(json!({
        "channelId": "ch_123",
        "pairingToken": "pt_123",
        "pairingExpiresAt": "2026-06-30T15:00:00Z"
    }))
    .expect("backend register ack shape should parse");

    assert_eq!(response.channel_id, "ch_123");
    assert_eq!(response.pairing_token, "pt_123");
    assert_eq!(response.pairing_expires_at, "2026-06-30T15:00:00Z");
}

#[test]
fn build_core_connect_payload_omits_session_token_for_core_role() {
    let payload = build_core_connect_payload("ch_123");

    assert_eq!(payload["channelId"], "ch_123");
    assert_eq!(payload["role"], "core");
    assert!(payload.get("sessionToken").is_none());
    assert!(payload.get("pairingToken").is_none());
}
