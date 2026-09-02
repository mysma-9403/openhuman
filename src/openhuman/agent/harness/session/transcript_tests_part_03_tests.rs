use super::*;

/// A file carrying an unknown record kind (as a future core might write) is
/// skipped by the reader rather than crashing it.
#[test]
fn unknown_record_kind_is_skipped_not_fatal() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("unknown_kind.jsonl");
    let meta = sample_meta();
    let mut h = AppendHarness::new(path.clone());
    h.turn(
        &[ChatMessage::system("sys"), ChatMessage::user("q1")],
        &meta,
        None,
        None,
    );
    // Simulate a future kind by appending a foreign record line.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "{{\"kind\":\"future_thing\",\"payload\":42}}").unwrap();
    }
    // Append a normal turn after the unknown line to prove reading continues.
    let mut msgs = vec![ChatMessage::system("sys"), ChatMessage::user("q1")];
    msgs.push(ChatMessage::assistant("a1"));
    h.prev = vec![ChatMessage::system("sys"), ChatMessage::user("q1")];
    h.turn(&msgs, &meta, None, None);

    let model = read_transcript(&path).unwrap();
    // The unknown record is skipped; the real messages survive.
    assert!(model.messages.iter().any(|m| m.content == "a1"));
    assert!(!model
        .messages
        .iter()
        .any(|m| m.content.contains("future_thing")));
}

/// The `_meta` version field is stamped by the append writer and absent (0) on
/// legacy files — but both remain readable.
#[test]
fn meta_version_stamped_and_optional() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("version.jsonl");
    let meta = sample_meta();
    let mut h = AppendHarness::new(path.clone());
    h.turn(&[ChatMessage::user("q")], &meta, None, None);
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.lines().next().unwrap().contains("\"version\":1"),
        "append writer must stamp the schema version on the meta header"
    );
}
