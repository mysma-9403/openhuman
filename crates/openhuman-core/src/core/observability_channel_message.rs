//! Sentry filter for expected missing channel-message relay rows.

/// 404 on PATCH/DELETE to a channel-message path is an expected backend state
/// (user deleted the message provider-side, backend GC'd the relay row). The
/// primary suppression lives in `authed_json` via `parse_message_path` +
/// defense-in-depth inline check. This filter is the outermost safety net for
/// any future call site that bypasses both. Targets OPENHUMAN-TAURI-R7.
///
/// Match criteria (all required):
/// - tag `domain == "backend_api"`
/// - tag `failure == "non_2xx"`
/// - tag `status == "404"`
/// - tag `method == "PATCH"` or `"DELETE"`
/// - event message or exception value contains both `"/channels/"` and `"/messages/"`
#[cfg(feature = "crash-reporting")]
pub fn is_channel_message_not_found_event(event: &sentry::protocol::Event<'_>) -> bool {
    let tags = &event.tags;
    if tags.get("domain").map(String::as_str) != Some("backend_api") {
        return false;
    }
    if tags.get("failure").map(String::as_str) != Some("non_2xx") {
        return false;
    }
    if tags.get("status").map(String::as_str) != Some("404") {
        return false;
    }
    let method = tags.get("method").map(String::as_str).unwrap_or("");
    if method != "PATCH" && method != "DELETE" {
        return false;
    }
    event_contains_channel_message_path(event)
}

#[cfg(feature = "crash-reporting")]
fn event_contains_channel_message_path(event: &sentry::protocol::Event<'_>) -> bool {
    let has_pattern = |s: &str| s.contains("/channels/") && s.contains("/messages/");
    if event.message.as_deref().is_some_and(has_pattern) {
        return true;
    }
    event
        .exception
        .values
        .iter()
        .any(|exc| exc.value.as_deref().is_some_and(has_pattern))
}
