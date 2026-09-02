use super::*;

#[test]
fn cron_completed_produces_agents_notification() {
    let ev = DomainEvent::CronJobCompleted {
        job_id: "job-1".into(),
        success: true,
        output: "done".into(),
    };
    let n = event_to_notification(&ev).expect("should produce notification");
    assert_eq!(n.category, CoreNotificationCategory::Agents);
    assert_eq!(n.title, "Cron job completed");
    assert!(n.body.contains("job-1"));
}

#[test]
fn provider_api_key_rejected_produces_system_notification() {
    let ev = DomainEvent::ProviderApiKeyRejected {
        provider: "openrouter".into(),
        message: "openrouter rejected the API key (HTTP 401). Update your openrouter \
                  API key in Connections → API keys → LLM to restore it."
            .into(),
    };
    let n = event_to_notification(&ev).expect("should produce notification");
    assert_eq!(n.category, CoreNotificationCategory::System);
    assert_eq!(n.title, "API key rejected");
    assert!(n.body.contains("openrouter"));
    assert!(n.body.contains("Connections"));
    assert_eq!(n.deep_link.as_deref(), Some("/connections?tab=llm"));
    assert!(n.id.starts_with("provider-key-rejected:openrouter:"));
}

#[test]
fn cron_failed_uses_failure_title() {
    let ev = DomainEvent::CronJobCompleted {
        job_id: "job-1".into(),
        success: false,
        output: "error".into(),
    };
    let n = event_to_notification(&ev).unwrap();
    assert_eq!(n.title, "Cron job failed");
}

#[test]
fn successful_webhook_is_silent() {
    let ev = DomainEvent::WebhookProcessed {
        tunnel_id: "t".into(),
        skill_id: "s".into(),
        method: "POST".into(),
        path: "/p".into(),
        correlation_id: "c".into(),
        status_code: 200,
        elapsed_ms: 5,
        error: None,
    };
    assert!(event_to_notification(&ev).is_none());
}

#[test]
fn failed_webhook_produces_system_notification() {
    let ev = DomainEvent::WebhookProcessed {
        tunnel_id: "t".into(),
        skill_id: "skill-x".into(),
        method: "POST".into(),
        path: "/p".into(),
        correlation_id: "c".into(),
        status_code: 500,
        elapsed_ms: 12,
        error: Some("boom".into()),
    };
    let n = event_to_notification(&ev).unwrap();
    assert_eq!(n.category, CoreNotificationCategory::System);
    assert!(n.body.contains("skill-x"));
    assert!(n.body.contains("boom"));
}

#[test]
fn subagent_completed_produces_agents_notification() {
    let ev = DomainEvent::SubagentCompleted {
        parent_session: "p".into(),
        task_id: "t".into(),
        agent_id: "researcher".into(),
        elapsed_ms: 100,
        output_chars: 500,
        iterations: 3,
    };
    let n = event_to_notification(&ev).unwrap();
    assert_eq!(n.category, CoreNotificationCategory::Agents);
    assert!(n.body.contains("researcher"));
    assert!(n.body.contains("500"));
}

#[test]
fn subagent_failed_produces_agents_notification() {
    let ev = DomainEvent::SubagentFailed {
        parent_session: "p".into(),
        task_id: "t".into(),
        agent_id: "researcher".into(),
        error: "context window exceeded".into(),
    };
    let n = event_to_notification(&ev).unwrap();
    assert_eq!(n.category, CoreNotificationCategory::Agents);
    assert_eq!(n.title, "Sub-agent failed");
    assert!(n.body.contains("researcher"));
    assert!(n.body.contains("context window exceeded"));
}

#[test]
fn unrelated_events_return_none() {
    let ev = DomainEvent::AgentTurnCompleted {
        session_id: "s".into(),
        text_chars: 1,
        iterations: 1,
    };
    assert!(event_to_notification(&ev).is_none());
}

#[test]
fn notification_triaged_escalate_produces_agents_notification() {
    let ev = DomainEvent::NotificationTriaged {
        id: "n1".into(),
        provider: "slack".into(),
        action: "escalate".into(),
        importance_score: 0.9,
        latency_ms: 100,
        routed: true,
    };
    let n = event_to_notification(&ev).expect("should produce notification");
    assert_eq!(n.category, CoreNotificationCategory::Agents);
    assert!(n.body.contains("escalate"));
    assert!(n.deep_link.as_deref() == Some("/notifications"));
}

#[test]
fn notification_triaged_react_uses_follow_up_copy() {
    let ev = DomainEvent::NotificationTriaged {
        id: "n2".into(),
        provider: "discord".into(),
        action: "react".into(),
        importance_score: 0.7,
        latency_ms: 120,
        routed: true,
    };
    let n = event_to_notification(&ev).expect("should produce notification");
    assert_eq!(n.category, CoreNotificationCategory::Agents);
    assert!(n.body.contains("Routed for follow-up"));
}

#[test]
fn notification_triaged_drop_is_silent() {
    let ev = DomainEvent::NotificationTriaged {
        id: "n1".into(),
        provider: "gmail".into(),
        action: "drop".into(),
        importance_score: 0.1,
        latency_ms: 50,
        routed: false,
    };
    assert!(event_to_notification(&ev).is_none());
}

#[test]
fn notification_triaged_unrouted_escalate_is_silent() {
    let ev = DomainEvent::NotificationTriaged {
        id: "n1".into(),
        provider: "slack".into(),
        action: "escalate".into(),
        importance_score: 0.9,
        latency_ms: 100,
        routed: false,
    };
    assert!(event_to_notification(&ev).is_none());
}
