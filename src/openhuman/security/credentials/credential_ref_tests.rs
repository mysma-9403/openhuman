//! Tests for `credential_ref` parsing, redaction, and the pre-lookup gates.

use super::*;

/// The name used everywhere below. Distinctive enough that a substring search
/// for it cannot pass by accident.
const SECRET_NAME: &str = "supermemory-prod-key-9f3a";

#[test]
fn parses_the_canonical_spelling() {
    let r = CredentialRef::parse("keychain:supermemory").expect("canonical form parses");
    assert_eq!(r.scheme(), CredentialRefScheme::Keychain);
    assert_eq!(r.name(), "supermemory");
}

#[test]
fn trims_whitespace_around_both_halves() {
    // A hand-edited config.toml must resolve the same entry as the canonical
    // spelling rather than looking up a name with a leading space.
    for raw in [
        "  keychain:supermemory  ",
        "keychain: supermemory",
        "keychain :supermemory",
        "\tkeychain:\tsupermemory\n",
    ] {
        let r = CredentialRef::parse(raw).unwrap_or_else(|e| panic!("{raw:?} should parse: {e}"));
        assert_eq!(r.name(), "supermemory", "for input {raw:?}");
        assert_eq!(r.scheme(), CredentialRefScheme::Keychain);
    }
}

#[test]
fn scheme_is_case_insensitive_but_the_name_is_not() {
    for raw in ["KEYCHAIN:Prod", "KeyChain:Prod", "keychain:Prod"] {
        let r = CredentialRef::parse(raw).unwrap_or_else(|e| panic!("{raw:?} should parse: {e}"));
        assert_eq!(r.scheme(), CredentialRefScheme::Keychain);
        // The name is a keychain key and the backing store is case-sensitive,
        // so normalising it would look up the wrong entry.
        assert_eq!(r.name(), "Prod", "name must survive verbatim for {raw:?}");
    }
}

#[test]
fn only_the_first_colon_separates_the_scheme() {
    // A name is free to contain a colon; splitting on every ':' would truncate
    // it and silently resolve a different entry.
    let r = CredentialRef::parse("keychain:ns:sub:key").expect("colons in the name are allowed");
    assert_eq!(r.name(), "ns:sub:key");
}

#[test]
fn rejects_malformed_references() {
    use CredentialRefError::*;
    let cases: &[(&str, CredentialRefError)] = &[
        ("", Empty),
        ("   ", Empty),
        ("supermemory", MissingScheme),
        ("keychain:", EmptyName),
        ("keychain:   ", EmptyName),
        (
            "vault:supermemory",
            UnsupportedScheme {
                scheme: "vault".to_string(),
            },
        ),
        (
            "ENV:SUPERMEMORY",
            UnsupportedScheme {
                scheme: "env".to_string(),
            },
        ),
    ];
    for (raw, expected) in cases {
        let err = CredentialRef::parse(raw).expect_err(&format!("{raw:?} must not parse"));
        assert_eq!(&err, expected, "for input {raw:?}");
    }
}

#[test]
fn an_unsupported_scheme_is_named_but_the_entry_name_is_not() {
    // The scheme is fixed vocabulary and is useful to report; everything after
    // it is operator data and must not travel with the error.
    let err = CredentialRef::parse(&format!("vault:{SECRET_NAME}")).expect_err("vault is unknown");
    let rendered = err.to_string();
    assert!(
        rendered.contains("vault"),
        "scheme should be named: {rendered}"
    );
    assert!(
        !rendered.contains(SECRET_NAME),
        "entry name leaked into the error: {rendered}"
    );
}

#[test]
fn debug_redacts_the_name() {
    // Deriving Debug here would put the reference into every `{:?}` and panic
    // message; plan-memory.md §7 Tier 3 forbids that.
    let r = CredentialRef::parse(&format!("keychain:{SECRET_NAME}")).expect("parses");
    let rendered = format!("{r:?}");
    assert!(
        !rendered.contains(SECRET_NAME),
        "credential name leaked through Debug: {rendered}"
    );
    assert!(
        rendered.contains("<redacted>"),
        "redaction marker missing: {rendered}"
    );
    // The scheme is still visible — the point is to stay debuggable.
    assert!(
        rendered.contains("Keychain"),
        "scheme should survive: {rendered}"
    );
}

#[test]
fn no_error_display_can_carry_a_name_or_a_secret() {
    // This is the property `memory::binding`'s FallbackReason depends on: any
    // of these may be rendered into `subsystems_status`, which is pinned by
    // `fallback_reason_never_contains_credential_ref_or_endpoint`.
    let all = [
        CredentialRefError::Empty,
        CredentialRefError::MissingScheme,
        CredentialRefError::UnsupportedScheme {
            scheme: "vault".to_string(),
        },
        CredentialRefError::EmptyName,
        CredentialRefError::ConsentPending,
        CredentialRefError::Unavailable,
        CredentialRefError::NotFound,
        CredentialRefError::Backend,
    ];
    for err in all {
        let rendered = err.to_string();
        assert!(
            !rendered.contains(SECRET_NAME),
            "{err:?} rendered a credential name: {rendered}"
        );
        assert!(!rendered.is_empty(), "{err:?} rendered an empty message");
    }
}

#[test]
fn preflight_reports_consent_before_availability() {
    // Order is the whole point. With consent pending AND no backend, the
    // operator must be told about consent — telling them the keychain is
    // unavailable would send them to fix the wrong thing.
    assert_eq!(
        preflight(PolicyDecision::ConsentRequired, false),
        Some(CredentialRefError::ConsentPending)
    );
    assert_eq!(
        preflight(PolicyDecision::Declined, false),
        Some(CredentialRefError::ConsentPending)
    );
}

#[test]
fn preflight_reports_unavailability_only_once_consent_is_granted() {
    assert_eq!(
        preflight(PolicyDecision::Proceed, false),
        Some(CredentialRefError::Unavailable)
    );
}

#[test]
fn preflight_allows_the_lookup_when_both_gates_pass() {
    assert_eq!(preflight(PolicyDecision::Proceed, true), None);
}

#[test]
fn preflight_never_reports_a_missing_entry() {
    // NotFound is a fact about the keychain's contents and can only be learned
    // by asking it. A gate that guessed it would report "not configured" for a
    // host that simply has no backend.
    for decision in [
        PolicyDecision::Proceed,
        PolicyDecision::ConsentRequired,
        PolicyDecision::Declined,
    ] {
        for available in [true, false] {
            assert_ne!(
                preflight(decision, available),
                Some(CredentialRefError::NotFound),
                "preflight invented a NotFound for {decision:?}/available={available}"
            );
        }
    }
}

#[test]
fn scheme_round_trips_through_its_wire_spelling() {
    assert_eq!(CredentialRefScheme::Keychain.as_str(), KEYCHAIN_SCHEME);
    let r = CredentialRef::parse(&format!("{KEYCHAIN_SCHEME}:x")).expect("parses");
    assert_eq!(r.scheme().as_str(), KEYCHAIN_SCHEME);
}
