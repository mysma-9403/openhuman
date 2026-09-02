use super::*;
use crate::openhuman::integrations::task_sources::types::{FilterSpec, ProviderSlug, SourceTarget};
use chrono::Utc;
use serde_json::json;

fn source(id: &str, interval_secs: u64) -> TaskSource {
    TaskSource {
        id: id.into(),
        provider: ProviderSlug::Github,
        connection_id: None,
        name: None,
        enabled: true,
        filter: FilterSpec::Github {
            repo: None,
            labels: vec![],
            assignee_is_me: true,
            state: None,
            fetch_mode: Default::default(),
            extra: json!({}),
        },
        interval_secs,
        target: SourceTarget::TodoOnly,
        max_tasks_per_fetch: 25,
        assigned_executor: None,
        created_at: Utc::now(),
        last_fetch_at: None,
        last_status: None,
    }
}

#[test]
fn tick_seconds_is_sane() {
    assert!(TICK_SECONDS >= 60);
    assert!(TICK_SECONDS <= 3600);
}

#[test]
fn never_polled_source_is_due() {
    let s = source("ts-never-polled-xyz", 1800);
    assert!(is_due(&s));
}

#[test]
fn recently_polled_source_is_not_due() {
    let s = source("ts-recent-poll-xyz", 1800);
    record_poll(&s.id);
    assert!(!is_due(&s), "just-recorded poll should not be due again");
}

#[test]
fn zero_interval_is_floored_not_always_due() {
    let s = source("ts-zero-interval-xyz", 0);
    record_poll(&s.id);
    // With the MIN_INTERVAL_SECONDS floor a just-polled zero-interval
    // source is not immediately due again.
    assert!(!is_due(&s));
}
