use super::*;
use serde_json::json;

#[tokio::test]
async fn stop_fences_the_driver_and_makes_a_running_phase_resumable() {
    let workspace = tempfile::tempdir().expect("workspace");
    let config = Config {
        workspace_dir: workspace.path().to_path_buf(),
        ..Config::default()
    };
    let run = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = WorkflowRunUpsert {
            id: "stop-resume-lifecycle".into(),
            definition_id: "parallel_research_cross_check".into(),
            parent_thread_id: None,
            input: json!({"question":"q"}),
            phase_states: json!({
                "decompose": {"status":"completed", "outputs": []},
                "research": {"status":"running", "outputs": []}
            }),
            child_run_ids: vec!["live-child".into()],
            status: WorkflowRunStatus::Running,
            summary: None,
            started_at: None,
            completed_at: None,
        };
        async move { ledger.upsert_workflow_run(ledger_arg_0).await }
    })
    .expect("seed run");

    let stopped = stop_workflow_run(&config, &run.id)
        .await
        .expect("stop succeeds")
        .expect("row exists");
    assert_eq!(stopped.status, WorkflowRunStatus::Interrupted);
    assert_eq!(stopped.phase_states["research"]["status"], json!("pending"));
    assert!(stopped.lease_owner.is_none());
    assert!(
        stopped.revision > run.revision,
        "stop must fence the old driver"
    );
}

#[tokio::test]
async fn stale_workflow_generation_cannot_fail_its_replacement() {
    let workspace = tempfile::tempdir().expect("workspace");
    let config = Config {
        workspace_dir: workspace.path().to_path_buf(),
        ..Config::default()
    };
    let run_id = format!("stale-generation-{}", uuid::Uuid::new_v4());
    let seeded = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let row = WorkflowRunUpsert {
            id: run_id.clone(),
            definition_id: "parallel_research_cross_check".into(),
            parent_thread_id: None,
            input: json!({"question":"q"}),
            phase_states: json!({"research":{"status":"pending","outputs":[]}}),
            child_run_ids: Vec::new(),
            status: WorkflowRunStatus::Running,
            summary: None,
            started_at: None,
            completed_at: None,
        };
        async move { ledger.upsert_workflow_run(row).await }
    })
    .expect("seed run");

    let supervisor = workflow_supervisor().clone();
    let old = supervisor.start_scoped_with_handle(
        run_id.clone(),
        |token| async move { token.cancelled().await },
        |future| future,
        |future| future,
        |_handle, future| {
            tokio::spawn(future);
        },
    );
    let current = supervisor.start_scoped_with_handle(
        run_id.clone(),
        |token| async move { token.cancelled().await },
        |future| future,
        |future| future,
        |_handle, future| {
            tokio::spawn(future);
        },
    );
    assert!(!supervisor.is_current(&old));
    assert!(supervisor.is_current(&current));

    let advanced = compare_and_swap_workflow_run_lifecycle(
        &config,
        WorkflowRunUpsert {
            id: seeded.id.clone(),
            definition_id: seeded.definition_id.clone(),
            parent_thread_id: seeded.parent_thread_id.clone(),
            input: seeded.input.clone(),
            phase_states: seeded.phase_states.clone(),
            child_run_ids: seeded.child_run_ids.clone(),
            status: WorkflowRunStatus::Running,
            summary: Some("replacement revision".into()),
            started_at: Some(seeded.started_at),
            completed_at: None,
        },
        seeded.revision,
    )
    .expect("advance workflow revision")
    .expect("the seeded revision is current");

    record_failed_if_current(
        &config,
        &run_id,
        &old,
        seeded.revision,
        "stale sentinel".into(),
    );
    record_failed_if_current(
        &config,
        &run_id,
        &current,
        seeded.revision,
        "stale revision sentinel".into(),
    );
    let unchanged = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let id = run_id.clone();
        async move { ledger.get_workflow_run(&id).await }
    })
    .expect("read row")
    .expect("run exists");
    assert_eq!(unchanged.revision, advanced.revision);
    assert_eq!(unchanged.summary.as_deref(), Some("replacement revision"));

    let stopped = stop_workflow_run(&config, &run_id)
        .await
        .expect("stop succeeds")
        .expect("run exists");
    assert_eq!(stopped.status, WorkflowRunStatus::Interrupted);
    assert!(current.cancellation_token().is_cancelled());
}

#[tokio::test]
async fn losing_resume_cas_keeps_the_installed_workflow_generation() {
    let workspace = tempfile::tempdir().expect("workspace");
    let config = Config {
        workspace_dir: workspace.path().to_path_buf(),
        ..Config::default()
    };
    let run_id = format!("resume-cas-race-{}", uuid::Uuid::new_v4());
    let interrupted = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let row = WorkflowRunUpsert {
            id: run_id.clone(),
            definition_id: "parallel_research_cross_check".into(),
            parent_thread_id: None,
            input: json!({"question":"q"}),
            phase_states: json!({"research":{"status":"pending","outputs":[]}}),
            child_run_ids: Vec::new(),
            status: WorkflowRunStatus::Interrupted,
            summary: None,
            started_at: None,
            completed_at: None,
        };
        async move { ledger.upsert_workflow_run(row).await }
    })
    .expect("seed interrupted run");

    let supervisor = workflow_supervisor().clone();
    let old = supervisor.start_scoped_with_handle(
        run_id.clone(),
        |token| async move { token.cancelled().await },
        |future| future,
        |future| future,
        |_handle, future| {
            tokio::spawn(future);
        },
    );
    let make_commit = || {
        let config = config.clone();
        let interrupted = interrupted.clone();
        move || {
            compare_and_swap_workflow_run_lifecycle(
                &config,
                WorkflowRunUpsert {
                    id: interrupted.id.clone(),
                    definition_id: interrupted.definition_id.clone(),
                    parent_thread_id: interrupted.parent_thread_id.clone(),
                    input: interrupted.input.clone(),
                    phase_states: interrupted.phase_states.clone(),
                    child_run_ids: interrupted.child_run_ids.clone(),
                    status: WorkflowRunStatus::Running,
                    summary: interrupted.summary.clone(),
                    started_at: Some(interrupted.started_at),
                    completed_at: None,
                },
                interrupted.revision,
            )?
            .ok_or_else(|| anyhow!("workflow run changed before resume"))
        }
    };
    let resume = |commit| {
        supervisor.resume_scoped_with_commit(
            run_id.clone(),
            commit,
            |token| async move { token.cancelled().await },
            |future| future,
            |future| future,
            |_handle, future| {
                tokio::spawn(future);
            },
        )
    };

    let (winner, resumed) = resume(make_commit()).expect("one resume commits");
    assert!(old.cancellation_token().is_cancelled());
    assert!(supervisor.is_current(&winner));
    assert_eq!(resumed.status, WorkflowRunStatus::Running);

    assert!(resume(make_commit()).is_err(), "stale resume CAS must lose");
    assert!(supervisor.is_current(&winner));
    assert!(
        !winner.cancellation_token().is_cancelled(),
        "a losing resume must not cancel the committed generation"
    );
}

#[tokio::test]
async fn stale_setup_failure_does_not_overwrite_another_process_lease() {
    use tinyagents_session::run_ledger::WorkflowLeaseClaim;

    let workspace = tempfile::tempdir().expect("workspace");
    let config = Config {
        workspace_dir: workspace.path().to_path_buf(),
        ..Config::default()
    };
    let run_id = format!("workflow-lease-owner-{}", uuid::Uuid::new_v4());
    let seeded = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let row = WorkflowRunUpsert {
            id: run_id.clone(),
            definition_id: "parallel_research_cross_check".into(),
            parent_thread_id: None,
            input: json!({"question":"q"}),
            phase_states: json!({"research":{"status":"pending","outputs":[]}}),
            child_run_ids: Vec::new(),
            status: WorkflowRunStatus::Running,
            summary: None,
            started_at: None,
            completed_at: None,
        };
        async move { ledger.upsert_workflow_run(row).await }
    })
    .expect("seed run");
    let claim = crate::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let id = run_id.clone();
        async move {
            ledger
                .try_claim_workflow_run(&id, "other-process", chrono::Duration::hours(1))
                .await
        }
    })
    .expect("claim workflow lease");
    let claimed = match claim {
        WorkflowLeaseClaim::Acquired(run) => run,
        other => panic!("expected lease acquisition, got {other:?}"),
    };

    let supervisor = workflow_supervisor().clone();
    let handle = supervisor.start_scoped_with_handle(
        run_id.clone(),
        |token| async move { token.cancelled().await },
        |future| future,
        |future| future,
        |_handle, future| {
            tokio::spawn(future);
        },
    );
    record_failed_if_current(
        &config,
        &run_id,
        &handle,
        seeded.revision,
        "stale setup failure".into(),
    );

    let after = get_workflow_run(&config, &run_id)
        .expect("read run")
        .expect("run remains present");
    assert_eq!(after.status, WorkflowRunStatus::Running);
    assert_eq!(after.lease_owner.as_deref(), Some("other-process"));
    assert_eq!(after.revision, claimed.revision);
    assert_ne!(after.summary.as_deref(), Some("stale setup failure"));
}
