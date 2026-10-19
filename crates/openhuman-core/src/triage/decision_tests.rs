use super::*;

// ── full parse_triage_decision ──────────────────────────────────────

#[test]
fn parses_clean_fenced_drop() {
    let reply =
        "Here's my verdict:\n```json\n{\"action\":\"drop\",\"reason\":\"duplicate event\"}\n```\n";
    let d = parse_triage_decision(reply).unwrap();
    assert_eq!(d.action, TriageAction::Drop);
    assert_eq!(d.reason, "duplicate event");
    assert!(d.target_agent.is_none());
    assert!(d.prompt.is_none());
}

#[test]
fn parses_unfenced_json_with_prose_before() {
    let reply = "I think this one needs human attention.\n\n{\"action\":\"escalate\",\"target_agent\":\"orchestrator\",\"prompt\":\"read the email and draft a reply\",\"reason\":\"complex request\"}";
    let d = parse_triage_decision(reply).unwrap();
    assert_eq!(d.action, TriageAction::Escalate);
    assert_eq!(d.target_agent.as_deref(), Some("orchestrator"));
    assert_eq!(
        d.prompt.as_deref(),
        Some("read the email and draft a reply")
    );
}

#[test]
fn parses_react_with_trailing_comma() {
    let reply = "```\n{\n  \"action\": \"react\",\n  \"target_agent\": \"trigger_reactor\",\n  \"prompt\": \"send ack\",\n  \"reason\": \"one-step ack needed\",\n}\n```";
    let d = parse_triage_decision(reply).unwrap();
    assert_eq!(d.action, TriageAction::React);
    assert_eq!(d.target_agent.as_deref(), Some("trigger_reactor"));
}

#[test]
fn parses_uppercase_action_field() {
    let reply = "{\"action\":\"DROP\",\"reason\":\"noise\"}";
    let d = parse_triage_decision(reply).unwrap();
    assert_eq!(d.action, TriageAction::Drop);
}

#[test]
fn rejects_escalate_without_target_agent() {
    let reply = "{\"action\":\"escalate\",\"reason\":\"complex\"}";
    let err = parse_triage_decision(reply).unwrap_err();
    assert!(matches!(
        err,
        ParseError::MissingTarget { action: "escalate" }
    ));
}

#[test]
fn rejects_react_without_prompt() {
    let reply = "{\"action\":\"react\",\"target_agent\":\"trigger_reactor\",\"reason\":\"x\"}";
    let err = parse_triage_decision(reply).unwrap_err();
    assert!(matches!(err, ParseError::MissingTarget { action: "react" }));
}

#[test]
fn rejects_reply_with_no_json_at_all() {
    let reply = "I don't feel like answering today";
    let err = parse_triage_decision(reply).unwrap_err();
    assert!(matches!(err, ParseError::NoJsonObject));
}

#[test]
fn rejects_non_parseable_json() {
    let reply = "{\"action\": not_a_string}";
    let err = parse_triage_decision(reply).unwrap_err();
    assert!(matches!(err, ParseError::InvalidJson(_)));
}

#[test]
fn prefers_last_fenced_block() {
    let reply = "```json\n{\"action\":\"escalate\",\"target_agent\":\"orchestrator\",\"prompt\":\"first\",\"reason\":\"a\"}\n```\nactually scratch that:\n```json\n{\"action\":\"drop\",\"reason\":\"never mind\"}\n```";
    let d = parse_triage_decision(reply).unwrap();
    assert_eq!(d.action, TriageAction::Drop);
    assert_eq!(d.reason, "never mind");
}

#[test]
fn owner_relaxed_json_recovery_preserves_action_normalization() {
    let reply = "{action: \"DROP\", reason: \"recovered\",}";
    let decision = parse_triage_decision(reply).unwrap();
    assert_eq!(decision.action, TriageAction::Drop);
    assert_eq!(decision.reason, "recovered");
}
