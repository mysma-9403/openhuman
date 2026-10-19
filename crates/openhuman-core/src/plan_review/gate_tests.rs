use super::*;

fn tenant() -> crate::core::runtime::Tenant {
    crate::core::runtime::Tenant::default()
}

#[tokio::test]
async fn approve_resolves_parked_turn() {
    let gate = PlanReviewGate::new(Duration::from_secs(5));
    let gate = std::sync::Arc::new(gate);
    let g2 = gate.clone();
    let parked = tokio::spawn(async move {
        g2.request_review(
            tenant(),
            Some("t1".into()),
            Some("c1".into()),
            "Ship it".into(),
            vec!["step one".into()],
            Some("call-1".into()),
        )
        .await
    });
    // Let the park register the waiter.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(gate.decide_by_thread(&tenant(), "t1", PlanReviewResolution::Approve));
    assert_eq!(parked.await.unwrap(), PlanReviewResolution::Approve);
}

#[tokio::test]
async fn revise_carries_feedback_back() {
    let gate = std::sync::Arc::new(PlanReviewGate::new(Duration::from_secs(5)));
    let g2 = gate.clone();
    let parked = tokio::spawn(async move {
        g2.request_review(
            tenant(),
            Some("t2".into()),
            None,
            "Plan".into(),
            vec![],
            None,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(gate.decide_by_thread(
        &tenant(),
        "t2",
        PlanReviewResolution::Revise {
            feedback: "add a test step".into(),
        },
    ));
    assert_eq!(
        parked.await.unwrap(),
        PlanReviewResolution::Revise {
            feedback: "add a test step".into(),
        }
    );
}

#[tokio::test]
async fn timeout_fails_closed_to_reject() {
    let gate = PlanReviewGate::new(Duration::from_millis(40));
    let resolution = gate
        .request_review(
            tenant(),
            Some("t3".into()),
            None,
            "Plan".into(),
            vec![],
            None,
        )
        .await;
    assert_eq!(resolution, PlanReviewResolution::Reject);
    // The waiter is cleaned up after timeout.
    assert!(!gate.decide_by_thread(&tenant(), "t3", PlanReviewResolution::Approve));
}

#[tokio::test]
async fn decide_unknown_request_is_false() {
    let gate = PlanReviewGate::new(Duration::from_secs(5));
    assert!(!gate.decide(&tenant(), "nope", PlanReviewResolution::Approve));
}

#[tokio::test]
async fn cancelled_park_cleans_up_waiter() {
    // A parked review whose future is dropped (turn cancel) before it
    // resolves must not leak its waiter / thread mapping.
    let gate = std::sync::Arc::new(PlanReviewGate::new(Duration::from_secs(30)));
    let g2 = gate.clone();
    let handle = tokio::spawn(async move {
        g2.request_review(
            tenant(),
            Some("t-drop".into()),
            None,
            "Plan".into(),
            vec![],
            None,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    handle.abort();
    let _ = handle.await;
    // The drop guard removed the entry, so there is nothing left to decide.
    assert!(!gate.decide_by_thread(&tenant(), "t-drop", PlanReviewResolution::Approve));
}

#[tokio::test]
async fn parked_review_for_thread_carries_tool_call_id_and_expiry() {
    let gate = std::sync::Arc::new(PlanReviewGate::new(Duration::from_secs(5)));
    let g2 = gate.clone();
    let parked = tokio::spawn(async move {
        g2.request_review(
            tenant(),
            Some("t-replay".into()),
            Some("c-replay".into()),
            "Ship it".into(),
            vec!["step one".into()],
            Some("call-replay".into()),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let row = gate
        .parked_review_for_thread(&tenant(), "t-replay")
        .expect("review should be parked");
    assert_eq!(row.thread_id, Some("t-replay".to_string()));
    assert_eq!(row.client_id, Some("c-replay".to_string()));
    assert_eq!(row.tool_call_id, Some("call-replay".to_string()));
    assert!(row.expires_at.is_some());
    assert_eq!(row.steps, vec!["step one".to_string()]);

    assert!(gate.decide_by_thread(&tenant(), "t-replay", PlanReviewResolution::Approve));
    parked.await.unwrap();

    // Decided reviews are no longer parked.
    assert!(gate
        .parked_review_for_thread(&tenant(), "t-replay")
        .is_none());
}

#[tokio::test]
async fn parked_review_for_thread_is_none_when_nothing_is_parked() {
    let gate = PlanReviewGate::new(Duration::from_secs(5));
    assert!(gate
        .parked_review_for_thread(&tenant(), "no-such-thread")
        .is_none());
}

#[tokio::test]
async fn parked_reviews_are_isolated_by_tenant() {
    let gate = std::sync::Arc::new(PlanReviewGate::new(Duration::from_secs(5)));
    let alice = crate::core::runtime::Tenant {
        profile: Some("review-alice".into()),
        agent: Some("trigger_reactor".into()),
    };
    let alice_ui = crate::core::runtime::Tenant {
        profile: Some("review-alice".into()),
        agent: None,
    };
    let bob = crate::core::runtime::Tenant {
        profile: Some("review-bob".into()),
        agent: None,
    };
    let parked = gate.request_review(
        alice.clone(),
        Some("same-thread".into()),
        Some("alice-client".into()),
        "Alice's plan".into(),
        vec!["private step".into()],
        None,
    );
    let mut parked = Box::pin(parked);
    assert!(futures::poll!(parked.as_mut()).is_pending());

    let alice_review = gate
        .parked_review_for_thread(&alice_ui, "same-thread")
        .expect("Alice's profile UI can replay its agent's review");
    assert!(gate.parked_review_for_thread(&bob, "same-thread").is_none());
    assert!(!gate.decide(
        &bob,
        &alice_review.request_id,
        PlanReviewResolution::Approve
    ));
    assert!(gate.decide(
        &alice_ui,
        &alice_review.request_id,
        PlanReviewResolution::Approve
    ));
    assert_eq!(parked.await, PlanReviewResolution::Approve);
}
