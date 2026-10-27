use super::*;

#[tokio::test]
async fn steer_returns_steer_outcome_with_instructions() {
    let (gate, _dir) = test_gate();
    let gate = Arc::new(gate);

    let g = gate.clone();
    let handle = tokio::spawn(async move {
        turn_origin::with_origin(
            web_origin(),
            g.intercept(
                &ApprovalTurnContext::chat(chat_ctx()),
                "shell",
                "git push --force origin main",
                serde_json::json!({}),
            ),
        )
        .await
    });

    let mut tries = 0;
    let pending = loop {
        let list = gate.list_pending().unwrap();
        if let Some(p) = list.into_iter().next() {
            break p;
        }
        tries += 1;
        assert!(tries < 50, "pending row never appeared");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    let directive = "Wypchnij to na osobny branch fix/login i otwórz PR";
    let decided = gate.steer(&pending.request_id, directive).unwrap();
    assert!(decided.is_some());

    let outcome = handle.await.unwrap();
    match outcome {
        GateOutcome::Steer { instructions } => {
            assert_eq!(instructions, directive);
        }
        other => panic!("expected GateOutcome::Steer, got {other:?}"),
    }
}

#[tokio::test]
async fn steer_publishes_domain_event_with_resolution() {
    crate::core::bus::init().await.expect("bus init");
    let mut rx = crate::core::bus::BUS
        .get()
        .expect("event bus initialized above")
        .receiver();
    let (gate, _dir) = test_gate();
    let gate = Arc::new(gate);

    let g = gate.clone();
    let handle = tokio::spawn(async move {
        turn_origin::with_origin(
            web_origin(),
            g.intercept(
                &ApprovalTurnContext::chat(chat_ctx()),
                "browser_click",
                "click pay button",
                serde_json::json!({}),
            ),
        )
        .await
    });

    let mut tries = 0;
    let pending = loop {
        let list = gate.list_pending().unwrap();
        if let Some(p) = list.into_iter().next() {
            break p;
        }
        tries += 1;
        assert!(tries < 50, "pending row never appeared");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    let directive = "Nie płać kartą X, użyj karty firmowej Y";
    gate.steer(&pending.request_id, directive).unwrap();

    let outcome = handle.await.unwrap();
    assert!(outcome.is_steer());

    let ev = find_approval_decided(&mut rx, &pending.request_id).await;
    match ev {
        crate::core::events::DomainEvent::ApprovalDecided {
            decision,
            resolution,
            ..
        } => {
            assert_eq!(decision, "steer");
            assert_eq!(resolution.as_deref(), Some("steered"));
        }
        other => panic!("expected ApprovalDecided event, got {other:?}"),
    }
}

#[tokio::test]
async fn steer_without_live_waiter_fails() {
    let (gate, _dir) = test_gate();
    let gate = Arc::new(gate);

    let request_id = "req-unparked-row";
    let pending = PendingApproval::new(
        request_id,
        "shell",
        "rm -rf /",
        serde_json::json!({}),
        None,
    );
    store::insert_pending(&gate.config, &pending, &gate.session_id).unwrap();

    let err = gate.steer(request_id, "do not run").unwrap_err();
    assert!(err.to_string().contains("no live waiter"));
}

#[tokio::test]
async fn steer_for_agent_enforces_agent_ownership() {
    let (gate, _dir) = test_gate();
    let gate = Arc::new(gate);

    let request_id = "req-test-ownership";
    let pending = PendingApproval::new(
        request_id,
        "shell",
        "rm -rf /",
        serde_json::json!({}),
        None,
    );
    store::insert_pending(&gate.config, &pending, &gate.session_id).unwrap();

    // Not registered to agent-b
    let err = gate.steer_for_agent("agent-b", request_id, "cancel this").unwrap_err();
    assert!(err.to_string().contains("belongs to another agent"));
}
