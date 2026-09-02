//! `credential_ref` resolution — the `[subsystems.*]` credential handle.
//!
//! `docs/specs/kernel.md` §3.6 gives every subsystem the same driver-binding
//! config shape, and `docs/specs/plan-memory.md` §4.5 fixes one field of it:
//!
//! ```toml
//! [subsystems.memory.drivers.supermemory]
//! credential_ref = "keychain:supermemory"
//! ```
//!
//! The value is a **reference**, never an inline secret. This module is the one
//! place that turns such a reference into the secret it names, so the rule
//! "config carries handles, the keychain carries secrets" has a single
//! enforcement point rather than one per subsystem. It lives in `security/`
//! rather than beside its first caller because the field is uniform across
//! subsystems by §3.6 — memory binds first, and inference, channels and sandbox
//! follow onto the same shape (kernel.md §5).
//!
//! ## Three refusals before the keychain is touched
//!
//! Resolution is deliberately not a bare [`keyring::get`]:
//!
//! 1. **Consent.** [`keyring_consent::policy::check_secret_access`] must return
//!    [`PolicyDecision::Proceed`]. A prompt that has not been answered is not a
//!    missing credential, and conflating the two would train an operator to
//!    "fix" a pending consent dialog by editing config.
//! 2. **Availability.** A host with no usable keychain backend reports that as
//!    itself rather than as a missing entry — the same order
//!    `web3::wallet`'s `keychain_load_mnemonic` uses.
//! 3. **Absence.** Only then is a `None` from the keychain a genuine
//!    "not configured".
//!
//! ## Nothing here may reach an operator-facing string
//!
//! [`CredentialRefError`] carries **no name and no secret**, and
//! [`CredentialRef`]'s `Debug` redacts the name. That is load-bearing rather
//! than decorative: `MemoryDriverConfig`'s own doc requires the credential
//! reference to stay out of `Debug`/error output (plan-memory.md §7, Tier 3),
//! and `memory::binding`'s `FallbackReason` is rendered into
//! `subsystems_status` — pinned by
//! `fallback_reason_never_contains_credential_ref_or_endpoint`.
//!
//! The sharp edge is [`KeyringError`], whose own `Display` interpolates the
//! key: `"OS keychain error for key '{key}': {source}"`. Propagating one
//! verbatim into a bind failure would leak the very name this module exists to
//! keep out of that string, so backend failures are deliberately mapped to a
//! name-free variant and the detail is logged instead.

use zeroize::Zeroizing;

use crate::openhuman::security::keyring;
use crate::openhuman::security::keyring_consent::{policy, PolicyDecision};

/// Log prefix for this module's diagnostics.
const LOG_PREFIX: &str = "[security:credential-ref]";

/// The `keychain:` scheme — the only one defined today.
///
/// Kept as a named constant because it is persisted in users' `config.toml`
/// and is therefore a compatibility surface, not an implementation detail.
pub const KEYCHAIN_SCHEME: &str = "keychain";

/// Why a `credential_ref` could not be parsed or resolved.
///
/// Every variant's `Display` is safe to place in an operator-facing string: it
/// names neither the credential nor the secret. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialRefError {
    /// The reference was empty or whitespace only.
    #[error("credential_ref is empty")]
    Empty,

    /// The reference had no `<scheme>:` prefix.
    #[error(
        "credential_ref is missing a '<scheme>:' prefix (expected \"{KEYCHAIN_SCHEME}:<name>\")"
    )]
    MissingScheme,

    /// The scheme is not one this build understands.
    ///
    /// The scheme itself is a fixed vocabulary word, not user data, so it is
    /// safe to name; the entry name after it is not and is never included.
    #[error("credential_ref scheme '{scheme}' is not supported (expected \"{KEYCHAIN_SCHEME}\")")]
    UnsupportedScheme {
        /// The offending scheme, lowercased.
        scheme: String,
    },

    /// The scheme was valid but the name after it was empty.
    #[error("credential_ref has an empty name after \"{KEYCHAIN_SCHEME}:\"")]
    EmptyName,

    /// Keychain access is gated behind a consent prompt that has not been
    /// answered. Distinct from [`Self::Unavailable`] on purpose.
    #[error("keychain access is pending user consent")]
    ConsentPending,

    /// No usable keychain backend on this host.
    #[error("no keychain backend is available on this host")]
    Unavailable,

    /// The keychain has no entry under this reference.
    #[error("no keychain entry matches this credential_ref")]
    NotFound,

    /// The keychain backend failed. Detail is logged, never rendered — see the
    /// module docs on [`KeyringError`](keyring::KeyringError).
    #[error("keychain lookup failed")]
    Backend,
}

/// The two gates that run *before* the keychain is touched, as a pure
/// function of what they observed.
///
/// Split out for the same reason `memory::binding::admit` is pure: the rule
/// worth pinning is the **order** — a pending consent prompt must be reported
/// as consent, not as an unavailable backend, and neither may be reported as a
/// missing entry. That ordering is what stops an operator "fixing" an
/// unanswered dialog by editing `config.toml`, and it is testable here without
/// a keychain, a consent store, or a booted core.
///
/// Returns `None` when the caller may proceed to the lookup.
#[must_use]
pub fn preflight(decision: PolicyDecision, keychain_available: bool) -> Option<CredentialRefError> {
    if decision != PolicyDecision::Proceed {
        return Some(CredentialRefError::ConsentPending);
    }
    if !keychain_available {
        return Some(CredentialRefError::Unavailable);
    }
    None
}

/// The scheme half of a parsed reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CredentialRefScheme {
    /// `keychain:<name>` — resolved through [`keyring`].
    Keychain,
}

impl CredentialRefScheme {
    /// The wire spelling, as it appears in `config.toml`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keychain => KEYCHAIN_SCHEME,
        }
    }
}

/// A parsed `credential_ref`.
///
/// Deliberately **not** `derive(Debug)` — see the manual impl below and the
/// module docs.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CredentialRef {
    scheme: CredentialRefScheme,
    name: String,
}

// Manual `Debug` that redacts the name. Deriving it would put the credential
// reference into every `format!("{ref:?}")`, `tracing::debug!(?r, ...)` and
// panic message, which plan-memory.md §7 Tier-3 forbids. This mirrors
// `MemoryDriverConfig`'s own manual redacting `Debug`. NEVER derive `Debug`.
impl std::fmt::Debug for CredentialRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialRef")
            .field("scheme", &self.scheme)
            .field("name", &"<redacted>")
            .finish()
    }
}

impl CredentialRef {
    /// Parse `"<scheme>:<name>"`.
    ///
    /// Surrounding whitespace is trimmed on both halves, so a hand-edited
    /// `config.toml` with `credential_ref = "keychain: supermemory"` resolves
    /// the same entry as the canonical spelling. The scheme is matched
    /// case-insensitively; the name is **not** normalised, because it is a
    /// keychain key and the backing store is case-sensitive.
    ///
    /// # Errors
    ///
    /// [`CredentialRefError::Empty`], [`CredentialRefError::MissingScheme`],
    /// [`CredentialRefError::UnsupportedScheme`] or
    /// [`CredentialRefError::EmptyName`]. None of them carry the name.
    pub fn parse(raw: &str) -> Result<Self, CredentialRefError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(CredentialRefError::Empty);
        }

        // `split_once` rather than `split(':')`: a name is free to contain a
        // colon, and only the first one separates the scheme.
        let Some((scheme_raw, name_raw)) = raw.split_once(':') else {
            return Err(CredentialRefError::MissingScheme);
        };

        let scheme_norm = scheme_raw.trim().to_ascii_lowercase();
        let scheme = if scheme_norm == KEYCHAIN_SCHEME {
            CredentialRefScheme::Keychain
        } else {
            return Err(CredentialRefError::UnsupportedScheme {
                scheme: scheme_norm,
            });
        };

        let name = name_raw.trim();
        if name.is_empty() {
            return Err(CredentialRefError::EmptyName);
        }

        Ok(Self {
            scheme,
            name: name.to_string(),
        })
    }

    /// The scheme this reference names.
    #[must_use]
    pub fn scheme(&self) -> CredentialRefScheme {
        self.scheme
    }

    /// The entry name, without the scheme prefix.
    ///
    /// Callers must treat this as sensitive: it is the one field this type's
    /// `Debug` deliberately hides, so do not interpolate it into a string that
    /// can reach an operator (see the module docs).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Resolve this reference to the secret it names, under `user_id`.
    ///
    /// `user_id` is the keychain partition the caller binds under; it is a
    /// parameter rather than something derived here so this module stays free
    /// of any one subsystem's notion of identity.
    ///
    /// The returned secret is wrapped in [`Zeroizing`] so it is wiped when the
    /// caller drops it rather than lingering in freed memory.
    ///
    /// # Errors
    ///
    /// [`CredentialRefError::ConsentPending`], [`CredentialRefError::Unavailable`],
    /// [`CredentialRefError::NotFound`] or [`CredentialRefError::Backend`], in
    /// that order of checking. No variant carries the name or the secret.
    pub fn resolve(&self, user_id: &str) -> Result<Zeroizing<String>, CredentialRefError> {
        match self.scheme {
            CredentialRefScheme::Keychain => self.resolve_keychain(user_id),
        }
    }

    fn resolve_keychain(&self, user_id: &str) -> Result<Zeroizing<String>, CredentialRefError> {
        if let Some(refusal) = preflight(policy::check_secret_access(), keyring::is_available()) {
            log::debug!(
                "{LOG_PREFIX} keychain access refused before lookup user_id={user_id} \
                 refusal={refusal} backend={}",
                keyring::backend_name()
            );
            return Err(refusal);
        }

        match keyring::get(user_id, &self.name) {
            Ok(Some(secret)) => {
                log::debug!("{LOG_PREFIX} resolved credential_ref user_id={user_id}");
                Ok(Zeroizing::new(secret))
            }
            Ok(None) => {
                log::debug!("{LOG_PREFIX} no keychain entry for credential_ref user_id={user_id}");
                Err(CredentialRefError::NotFound)
            }
            // Logged, not propagated: `KeyringError`'s Display interpolates the
            // key, and this error's Display is allowed into operator-facing
            // strings. See the module docs.
            Err(e) => {
                log::warn!("{LOG_PREFIX} keychain lookup failed user_id={user_id}: {e}");
                Err(CredentialRefError::Backend)
            }
        }
    }
}

#[cfg(test)]
#[path = "credential_ref_tests.rs"]
mod tests;
