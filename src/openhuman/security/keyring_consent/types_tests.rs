use super::*;

#[test]
fn storage_mode_serialization_roundtrip() {
    let modes = [
        StorageMode::OsKeyring,
        StorageMode::LocalEncrypted,
        StorageMode::ConsentPending,
        StorageMode::Declined,
    ];
    for mode in modes {
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: StorageMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }
}

#[test]
fn storage_mode_display() {
    assert_eq!(StorageMode::OsKeyring.to_string(), "os_keyring");
    assert_eq!(StorageMode::ConsentPending.to_string(), "consent_pending");
}

#[test]
fn failure_reason_display() {
    assert_eq!(
        KeyringFailureReason::NoSecretService.to_string(),
        "No Secret Service daemon available"
    );
    assert_eq!(
        KeyringFailureReason::Unknown("custom".to_string()).to_string(),
        "custom"
    );
}

#[test]
fn keyring_status_serialization() {
    let status = KeyringStatus {
        available: false,
        failure_reason: Some(KeyringFailureReason::NoSecretService),
        active_mode: StorageMode::ConsentPending,
        backend_name: "os".to_string(),
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["available"], false);
    assert_eq!(json["activeMode"], "consent_pending");
    assert_eq!(json["failureReason"], "no_secret_service");
}

#[test]
fn keyring_status_omits_none_failure_reason() {
    let status = KeyringStatus {
        available: true,
        failure_reason: None,
        active_mode: StorageMode::OsKeyring,
        backend_name: "os".to_string(),
    };
    let json = serde_json::to_value(&status).unwrap();
    assert!(!json.as_object().unwrap().contains_key("failureReason"));
}

#[test]
fn consent_preference_defaults() {
    let pref = ConsentPreference::default();
    assert_eq!(pref.storage_mode, "");
    assert!(pref.consented_at_ms.is_none());
}
