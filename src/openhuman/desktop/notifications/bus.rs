//! Broadcast bus + DomainEvent subscriber for core notifications.
//!
//! Mirrors the pattern used by [`overlay::bus`](crate::openhuman::desktop::overlay::bus)
//! — a single `tokio::sync::broadcast` channel wrapped in a `Lazy` static,
//! plus a [`EventHandler`] implementation that translates relevant
//! [`DomainEvent`] variants into [`CoreNotificationEvent`] payloads.
//!
//! The Socket.IO bridge in `core::socketio::spawn_web_channel_bridge`
//! subscribes to this bus and forwards every event to all connected clients
//! as `core_notification` / `core:notification` Socket.IO messages.

use once_cell::sync::Lazy;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use crate::core::events::DomainEvent;
use crate::openhuman::config::Config;
use async_trait::async_trait;
use tinybus::EventHandler;

use super::types::{CoreNotificationCategory, CoreNotificationEvent};

const LOG_PREFIX: &str = "[core-notify]";

static NOTIFICATION_BUS: Lazy<broadcast::Sender<CoreNotificationEvent>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(128);
    tx
});

/// Subscribe to core notifications — consumed by the Socket.IO bridge at
/// startup. Additional in-process consumers (e.g. integration tests) can
/// subscribe too.
pub fn subscribe_core_notifications() -> broadcast::Receiver<CoreNotificationEvent> {
    NOTIFICATION_BUS.subscribe()
}

/// Publish a core notification. Fire-and-forget: if nobody is currently
/// subscribed the event is dropped. Returns the number of active
/// subscribers that received the event for diagnostics.
pub fn publish_core_notification(event: CoreNotificationEvent) -> usize {
    log::debug!(
        "{LOG_PREFIX} publish id={} category={:?} title_chars={}",
        event.id,
        event.category,
        event.title.len(),
    );
    NOTIFICATION_BUS.send(event).unwrap_or(0)
}

/// Subscribes to selected DomainEvent variants and translates each into a
/// [`CoreNotificationEvent`], persisting it (#3805) and broadcasting it to any
/// connected client.
///
/// `config` is `None` only in unit tests that exercise the pure translation /
/// subscriber-name contract without a workspace on disk; in production it is
/// always `Some`, so every core notification is durably stored before being
/// broadcast and therefore survives an app-closed / disconnected window.
#[derive(Default)]
pub struct NotificationBridgeSubscriber {
    config: Option<Config>,
}

impl NotificationBridgeSubscriber {
    /// Construct a subscriber that persists notifications to the workspace
    /// store backed by `config` before broadcasting them.
    pub fn new(config: Config) -> Self {
        Self {
            config: Some(config),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pure translation function — kept free so unit tests can drive it
/// without spinning up tokio or the broadcast channel.
pub fn event_to_notification(event: &DomainEvent) -> Option<CoreNotificationEvent> {
    let ts = now_ms();
    match event {
        DomainEvent::CronJobCompleted {
            job_id, success, ..
        } => Some(CoreNotificationEvent {
            id: format!("cron:{}:{}", job_id, ts),
            category: CoreNotificationCategory::Agents,
            title: if *success {
                "Cron job completed".into()
            } else {
                "Cron job failed".into()
            },
            body: if *success {
                format!("Job {job_id} finished successfully.")
            } else {
                format!("Job {job_id} did not complete — check your cron schedule.")
            },
            deep_link: Some("/settings/cron-jobs".into()),
            timestamp_ms: ts,
            actions: None,
        }),
        DomainEvent::WebhookProcessed {
            skill_id,
            status_code,
            elapsed_ms,
            error,
            ..
        } => {
            // Only surface failures — successful webhooks are noisy.
            if error.is_none() && *status_code < 400 {
                return None;
            }
            Some(CoreNotificationEvent {
                id: format!("webhook:{}:{}", skill_id, ts),
                category: CoreNotificationCategory::System,
                title: "Webhook error".into(),
                body: match error {
                    Some(err) => {
                        format!("{skill_id} webhook failed after {elapsed_ms}ms: {err}")
                    }
                    None => format!(
                        "{skill_id} webhook returned HTTP {status_code} after {elapsed_ms}ms"
                    ),
                },
                deep_link: Some("/settings/webhooks-triggers".into()),
                timestamp_ms: ts,
                actions: None,
            })
        }
        DomainEvent::SubagentCompleted {
            parent_session,
            task_id,
            agent_id,
            output_chars,
            ..
        } => Some(CoreNotificationEvent {
            id: format!("subagent:{}:{}:{}", parent_session, task_id, ts),
            category: CoreNotificationCategory::Agents,
            title: "Sub-agent finished".into(),
            body: format!("{agent_id} produced {output_chars} chars of output."),
            deep_link: Some("/chat".into()),
            timestamp_ms: ts,
            actions: None,
        }),
        DomainEvent::SubagentFailed {
            parent_session,
            task_id,
            agent_id,
            error,
        } => Some(CoreNotificationEvent {
            id: format!("subagent:{}:{}:{}", parent_session, task_id, ts),
            category: CoreNotificationCategory::Agents,
            title: "Sub-agent failed".into(),
            body: format!(
                "{agent_id} encountered an error: {}",
                error.chars().take(100).collect::<String>()
            ),
            deep_link: Some("/chat".into()),
            timestamp_ms: ts,
            actions: None,
        }),
        DomainEvent::NotificationTriaged {
            id,
            provider,
            action,
            importance_score,
            latency_ms,
            routed,
        } if *routed && (action == "escalate" || action == "react") => {
            Some(CoreNotificationEvent {
                id: format!("notification-triaged:{}:{}:{}", id, action, latency_ms),
                category: CoreNotificationCategory::Agents,
                title: format!("High-priority {} notification", provider),
                body: if action == "escalate" {
                    format!(
                        "Action: escalate (score: {:.0}%). Routed to orchestrator.",
                        importance_score * 100.0
                    )
                } else {
                    format!(
                        "Action: react (score: {:.0}%). Routed for follow-up.",
                        importance_score * 100.0
                    )
                },
                deep_link: Some("/notifications".into()),
                timestamp_ms: ts,
                actions: None,
            })
        }
        DomainEvent::ProviderApiKeyRejected { provider, message } => Some(CoreNotificationEvent {
            id: format!("provider-key-rejected:{}:{}", provider, ts),
            category: CoreNotificationCategory::System,
            title: "API key rejected".into(),
            // `message` is already a pre-formatted, actionable string from
            // `auth_error_registry::auth_error_message`.
            body: message.clone(),
            // Land the user on the AI-settings LLM tab, where the inline
            // provider-error notice + key editor live. Must be the canonical
            // `/connections?tab=llm` route: `/skills` is a back-compat
            // redirect that drops the query and defaults to the Apps tab, so
            // it would not surface the key editor.
            deep_link: Some("/connections?tab=llm".into()),
            timestamp_ms: ts,
            actions: None,
        }),
        _ => None,
    }
}

#[async_trait]
impl EventHandler<DomainEvent> for NotificationBridgeSubscriber {
    fn name(&self) -> &str {
        "notifications::bridge"
    }

    // `domains()` returns None — we filter at the variant match instead of
    // the domain string, since we pull from three different domains and
    // the domain list is an optional short-circuit rather than a
    // correctness boundary.

    async fn handle(&self, event: &DomainEvent) {
        if let Some(notification) = event_to_notification(event) {
            // #3805: persist BEFORE broadcasting so the event is durable even
            // when no client is currently subscribed (app closed / minimised /
            // disconnected) — otherwise the broadcast send finds zero
            // receivers and the notification is lost forever. Best-effort: a
            // store failure must not suppress the live broadcast.
            if let Some(config) = &self.config {
                match super::store::insert_core_notification(config, &notification) {
                    Ok(true) => log::debug!(
                        "{LOG_PREFIX} persisted core notification id={}",
                        notification.id
                    ),
                    Ok(false) => log::debug!(
                        "{LOG_PREFIX} core notification id={} already persisted (dedup)",
                        notification.id
                    ),
                    Err(err) => log::warn!(
                        "{LOG_PREFIX} failed to persist core notification id={}: {err}",
                        notification.id
                    ),
                }
            }
            publish_core_notification(notification);
        }
    }
}

/// Register the notification bridge subscriber on the global event bus.
/// Safe to call multiple times — each call produces a fresh subscription,
/// but the caller (`register_domain_subscribers`) is Once-guarded.
pub fn register_notification_bridge_subscriber(config: Config) {
    use std::sync::Arc;
    if let Some(handle) =
        crate::core::bus::BUS.subscribe(Arc::new(NotificationBridgeSubscriber::new(config)))
    {
        // SAFETY: intentional leak; handle's Drop would cancel the subscriber.
        std::mem::forget(handle);
        log::info!("{LOG_PREFIX} notification bridge subscriber registered");
    } else {
        log::warn!(
            "{LOG_PREFIX} failed to register notification bridge — event bus not initialized"
        );
    }
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod bus_tests;

#[cfg(test)]
#[path = "bus_tests_2_tests.rs"]
mod tests;
