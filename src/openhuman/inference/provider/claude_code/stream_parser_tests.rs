use super::*;

#[test]
fn parses_multiline_chunk() {
    let mut p = StreamJsonParser::new();
    let chunk = r#"{"type":"system","session_id":"s1","schema_version":"2.0"}
{"type":"assistant","message":{"type":"content_block_start","index":0,"content_block":{"type":"text"}}}
"#;
    let events = p.feed(chunk);
    assert_eq!(events.len(), 2);
    assert_eq!(p.schema_version.as_deref(), Some("2.0"));
    assert!(matches!(events[0], ClaudeCodeEvent::System { .. }));
    assert!(matches!(events[1], ClaudeCodeEvent::Assistant { .. }));
}

#[test]
fn handles_split_lines_across_chunks() {
    let mut p = StreamJsonParser::new();
    assert!(p.feed("{\"type\":\"system\"").is_empty());
    assert!(p.feed(",\"session_id\":\"s1\"}").is_empty());
    let events = p.feed("\n");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ClaudeCodeEvent::System { .. }));
}

#[test]
fn flushes_trailing_line_on_end() {
    let mut p = StreamJsonParser::new();
    assert!(p
        .feed(r#"{"type":"result","subtype":"success"}"#)
        .is_empty());
    let events = p.end();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ClaudeCodeEvent::Result { .. }));
}

#[test]
fn unknown_type_becomes_parse_error() {
    let mut p = StreamJsonParser::new();
    let events = p.feed("{\"type\":\"weird\"}\n");
    assert!(matches!(events[0], ClaudeCodeEvent::ParseError { .. }));
}

#[test]
fn bad_json_becomes_parse_error() {
    let mut p = StreamJsonParser::new();
    let events = p.feed("not json\n");
    assert!(matches!(events[0], ClaudeCodeEvent::ParseError { .. }));
}
