use super::*;

fn msg(role: &str, content: &str) -> ChatMessage {
    match role {
        "system" => ChatMessage::system(content),
        "user" => ChatMessage::user(content),
        "assistant" => ChatMessage::assistant(content),
        _ => ChatMessage::tool(content),
    }
}

#[test]
fn new_session_pipes_full_user_history() {
    let history = vec![
        msg("system", "you are helpful"),
        msg("user", "hi"),
        msg("assistant", "hello"),
        msg("user", "how are you?"),
    ];
    let bytes = build_stdin(&history, true);
    let s = String::from_utf8(bytes).unwrap();
    let lines: Vec<_> = s.lines().collect();
    assert_eq!(lines.len(), 3); // system filtered out
    assert!(lines[0].contains("\"hi\""));
    assert!(lines[1].contains("\"hello\""));
    assert!(lines[2].contains("how are you"));
}

#[test]
fn resume_pipes_only_last_user_turn() {
    let history = vec![
        msg("user", "earlier turn"),
        msg("assistant", "earlier reply"),
        msg("user", "follow-up"),
    ];
    let bytes = build_stdin(&history, false);
    let s = String::from_utf8(bytes).unwrap();
    let lines: Vec<_> = s.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"follow-up\""));
}

#[test]
fn empty_history_yields_empty_bytes() {
    let bytes = build_stdin(&[], true);
    assert!(bytes.is_empty());
}
