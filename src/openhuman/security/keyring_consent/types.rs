//! Types for the keyring consent domain.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMode {
    OsKeyring,
    LocalEncrypted,
    ConsentPending,
    Declined,
}

impl std::fmt::Display for StorageMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OsKeyring => write!(f, "os_keyring"),
            Self::LocalEncrypted => write!(f, "local_encrypted"),
            Self::ConsentPending => write!(f, "consent_pending"),
            Self::Declined => write!(f, "declined"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyringFailureReason {
    NoSecretService,
    KeychainLocked,
    AccessDenied,
    MasterKeyUnavailable,
    Unknown(String),
}

impl std::fmt::Display for KeyringFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSecretService => write!(f, "No Secret Service daemon available"),
            Self::KeychainLocked => write!(f, "OS keychain is locked"),
            Self::AccessDenied => write!(f, "Access to OS keychain was denied"),
            Self::MasterKeyUnavailable => write!(f, "Master encryption key unavailable"),
            Self::Unknown(msg) => write!(f, "{msg}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyringStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<KeyringFailureReason>,
    pub active_mode: StorageMode,
    pub backend_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConsentPreference {
    #[serde(default)]
    pub storage_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consented_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Proceed,
    ConsentRequired,
    Declined,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
