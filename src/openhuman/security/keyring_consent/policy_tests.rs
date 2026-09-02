use super::*;
use std::sync::{Mutex, OnceLock};

fn cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("keyring consent cache test lock")
}

#[test]
fn classify_failure_linux() {
    if cfg!(target_os = "linux") {
        let reason = classify_failure_reason("os");
        assert_eq!(reason, KeyringFailureReason::NoSecretService);
    }
}

#[test]
fn classify_failure_macos() {
    if cfg!(target_os = "macos") {
        let reason = classify_failure_reason("os");
        assert_eq!(reason, KeyringFailureReason::AccessDenied);
    }
}

#[test]
fn classify_failure_encrypted_file() {
    let reason = classify_failure_reason("encrypted_file");
    assert_eq!(reason, KeyringFailureReason::MasterKeyUnavailable);
}

#[test]
fn classify_failure_unknown() {
    let reason = classify_failure_reason("weird_backend");
    assert!(matches!(reason, KeyringFailureReason::Unknown(_)));
}

#[test]
fn record_consent_updates_cache() {
    let _lock = cache_test_lock();
    let pref = record_consent("local_encrypted");
    assert_eq!(pref.storage_mode, "local_encrypted");
    assert!(pref.consented_at_ms.is_some());

    let cached = CONSENT_CACHE.read().clone();
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().storage_mode, "local_encrypted");
}

#[test]
fn initialize_populates_cache() {
    let _lock = cache_test_lock();
    *CONSENT_CACHE.write() = None;
    let pref = ConsentPreference {
        storage_mode: "declined".to_string(),
        consented_at_ms: Some(12345),
    };
    initialize(Some(pref.clone()));
    let cached = CONSENT_CACHE.read().clone();
    assert_eq!(cached.unwrap().storage_mode, "declined");
}

#[test]
fn initialize_is_change_gated() {
    let _lock = cache_test_lock();
    *CONSENT_CACHE.write() = None;

    // First real value populates the cache and reports it applied (the INFO
    // log + write happened).
    let pref = ConsentPreference {
        storage_mode: "local_encrypted".to_string(),
        consented_at_ms: Some(111),
    };
    assert!(initialize(Some(pref.clone())), "first value should apply");
    assert_eq!(CONSENT_CACHE.read().clone(), Some(pref.clone()));

    // Repeat with the identical value — the no-op path: returns false (no
    // write, no INFO log), which is what every app_state_snapshot hits.
    // Asserting the return value proves the side effect is suppressed, not
    // merely that the resulting cache value is unchanged.
    assert!(
        !initialize(Some(pref.clone())),
        "identical value must be a no-op (no re-log / re-write)"
    );
    assert_eq!(CONSENT_CACHE.read().clone(), Some(pref));

    // A genuine change is still applied (returns true).
    let changed = ConsentPreference {
        storage_mode: "declined".to_string(),
        consented_at_ms: Some(222),
    };
    assert!(
        initialize(Some(changed.clone())),
        "a genuine change should apply"
    );
    assert_eq!(CONSENT_CACHE.read().clone(), Some(changed));
}
