//! Unit tests for the guided setup handlers.
//!
//! The handle parsing is what is testable without a running service, and it is
//! the part that matters: a handle the agent invented must be refused with a
//! message naming it, not silently looked up and missed.

use super::*;

#[test]
fn a_handle_map_is_parsed() {
    let parsed = parse_handles(HashMap::from([(
        "API_KEY".to_string(),
        "secret://abc123".to_string(),
    )]))
    .expect("a valid handle");

    assert_eq!(parsed["API_KEY"].as_str(), "secret://abc123");
}

#[test]
fn a_bare_handle_is_accepted() {
    // A caller may pass back either form.
    let parsed = parse_handles(HashMap::from([(
        "API_KEY".to_string(),
        "abc123".to_string(),
    )]))
    .expect("a bare handle");

    assert_eq!(parsed["API_KEY"].as_str(), "secret://abc123");
}

#[test]
fn an_invented_handle_is_refused_and_named() {
    let error = parse_handles(HashMap::from([(
        "API_KEY".to_string(),
        "not-a-handle".to_string(),
    )]))
    .expect_err("an invented handle");

    assert!(error.contains("not-a-handle"), "{error}");
}

#[test]
fn an_empty_handle_map_parses_to_nothing() {
    assert!(parse_handles(HashMap::new()).unwrap().is_empty());
}
