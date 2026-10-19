use super::*;

const TINY_PNG_DATA_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

#[test]
fn for_untrusted_channel_input_disables_file_markers_and_remote_fetch() {
    let config = crate::config::MultimodalFileConfig::for_untrusted_channel_input();
    assert_eq!(config.max_files, 0);
    assert!(!config.allow_remote_fetch);
}

#[tokio::test]
async fn stash_image_attachments_replaces_marker_with_placeholder() {
    let msg = format!("whats this [IMAGE:{TINY_PNG_DATA_URI}]");
    let out = stash_image_attachments(&msg, &MultimodalConfig::default()).await;
    assert!(out.contains("[Image:"), "placeholder present: {out}");
    assert!(out.contains("#att:"), "stash ref present: {out}");
    assert!(!out.contains("[IMAGE:data:"), "raw marker removed: {out}");
    assert!(!out.contains("base64"), "base64 not persisted: {out}");
    assert!(out.contains("whats this"), "user text preserved: {out}");
}

#[tokio::test]
async fn image_placeholder_rehydrates_to_inline_data_uri() {
    let msg = format!("describe [IMAGE:{TINY_PNG_DATA_URI}]");
    let placeholdered = stash_image_attachments(&msg, &MultimodalConfig::default()).await;
    let messages = vec![TranscriptMessage::user(placeholdered)];
    assert!(has_image_placeholders(&messages));

    let rehydrated = rehydrate_image_placeholders(&messages).await;
    assert_eq!(rehydrated.len(), 1);
    let content = &rehydrated[0].content;
    assert!(
        content.contains("[IMAGE:data:image/png;base64,"),
        "{content}"
    );
    assert!(
        !content.contains("#att:"),
        "placeholder consumed: {content}"
    );
}

#[tokio::test]
async fn rehydrate_missing_stash_id_keeps_placeholder_text() {
    let messages = vec![TranscriptMessage::user(
        "see [Image: image #att:deadbeefdeadbeef]".to_string(),
    )];
    let out = rehydrate_image_placeholders(&messages).await;
    assert!(out[0]
        .content
        .contains("[Image: image #att:deadbeefdeadbeef]"));
    assert!(!out[0].content.contains("[IMAGE:data:"));
}

#[tokio::test]
async fn stash_image_attachments_caps_at_max_images() {
    let msg = format!("[IMAGE:{TINY_PNG_DATA_URI}]\n[IMAGE:{TINY_PNG_DATA_URI}]");
    let config = MultimodalConfig {
        max_images: 1,
        ..MultimodalConfig::default()
    };
    let out = stash_image_attachments(&msg, &config).await;
    assert!(out.contains("#att:"), "first image stashed: {out}");
    assert!(
        out.contains("over image limit"),
        "second image capped: {out}"
    );
}

#[test]
fn extract_image_placeholders_pulls_attachment_tokens_in_order() {
    let text = "look [Image: image #att:aaa111] and [Image: image #att:bbb222]";
    assert_eq!(
        markers::extract_image_placeholders_in_text(text),
        vec![
            "[Image: image #att:aaa111]".to_string(),
            "[Image: image #att:bbb222]".to_string()
        ]
    );
    assert!(
        markers::extract_image_placeholders_in_text("[Image: (could not be processed)]").is_empty()
    );
    assert!(markers::extract_image_placeholders_in_text("no images here").is_empty());
}

#[tokio::test]
async fn sweep_keeps_fresh_attachments() {
    let msg = format!("[IMAGE:{TINY_PNG_DATA_URI}]");
    let placeholdered = stash_image_attachments(&msg, &MultimodalConfig::default()).await;
    let id = placeholdered
        .split("#att:")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .map(str::trim)
        .expect("placeholder carries an id")
        .to_owned();

    sweep_stale_attachments().await;

    let index = build_attachment_index(&[TranscriptMessage::user(placeholdered)]).await;
    assert!(
        index.contains_key(&id),
        "fresh attachment retained after sweep"
    );
}

#[test]
fn legacy_sidecar_files_stay_indexed_by_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("abc123.png"), b"png").unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("bad name.png"), b"png").unwrap();
    let index = legacy_attachment_index_in(dir.path());
    assert_eq!(index.len(), 1, "{index:?}");
    assert_eq!(index["abc123"], dir.path().join("abc123.png"));
}
