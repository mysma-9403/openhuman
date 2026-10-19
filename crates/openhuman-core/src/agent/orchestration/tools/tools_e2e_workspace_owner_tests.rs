use super::*;

#[tokio::test]
async fn driver_job_controls_are_isolated_by_workspace_with_shared_runtime_state() {
    use crate::agent::orchestration::subagent_sessions::{
        DurableSubagentSession, DurableSubagentStatus, SubagentSessionStore,
    };
    use crate::agent::subagent_host::{run_subagent_via_driver_tool, SubagentRunOptions};
    use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet};
    use crate::tools::toolpacks::ToolGroups;
    use tinyagents_harness::agent_loop::phases::execute_tool_batch;
    use tinyagents_orchestration::subagent::SubAgentJobRegistry;
    use tinyagents_runtime::turn_driver::SharedToolSet;

    struct PausedModel {
        started: tokio::sync::mpsc::UnboundedSender<()>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl ChatModel<()> for PausedModel {
        async fn invoke(
            &self,
            _state: &(),
            _request: ModelRequest,
        ) -> tinyinference_llm::Result<ModelResponse> {
            let _ = self.started.send(());
            self.release.notified().await;
            Ok(ModelResponse::assistant("workspace child finished"))
        }
    }

    let scratch = tempfile::tempdir().expect("workspace owner isolation fixture");
    let workspace_a = scratch.path().join("workspace-a");
    let workspace_b = scratch.path().join("workspace-b");
    std::fs::create_dir_all(&workspace_a).expect("create workspace A");
    std::fs::create_dir_all(&workspace_b).expect("create workspace B");
    let mut base_config = crate::config::Config::default();
    base_config.workspace_dir = workspace_a.clone();
    base_config.components.juice = false;
    let base = CoreContext::for_test_with_config(DomainSet::full(), base_config.clone());
    let definitions = Arc::new(AgentDefinitionRegistry::builtins_only());
    let mut config_a = base_config.clone();
    config_a.workspace_dir = workspace_a.clone();
    let mut config_b = base_config;
    config_b.workspace_dir = workspace_b.clone();
    let core_a = base.derive_with(
        ContextOverlay::new(config_a, DomainSet::full(), ToolGroups::none())
            .definitions(definitions.clone()),
    );
    let core_b = base.derive_with(
        ContextOverlay::new(config_b, DomainSet::full(), ToolGroups::none())
            .definitions(definitions),
    );

    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let model: Arc<dyn ChatModel<()>> = Arc::new(PausedModel {
        started: started_tx,
        release: release.clone(),
    });
    let build_parent = |workspace: &std::path::Path, core: &Arc<CoreContext>| {
        let mut product_parent = parent_context(workspace, model.clone(), vec![]);
        product_parent.workspace_dir = workspace.to_path_buf();
        product_parent.session_id = "same-parent-session".into();
        let mut options = SubagentRunOptions::default();
        options.run_context.parent = Some(product_parent);
        options.task_id = Some("same-raw-task-id".into());
        options.thread_id = Some("same-parent-thread".into());
        options.run_context.thread_id = Some("same-parent-thread".into());
        options.run_context = options.run_context.with_core(Some(Arc::clone(core)));
        let run_id = workspace
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("workspace");
        let run = options.run_context.clone().into_tinyagents(
            tinyagents_harness::context::RunConfig::new(format!("parent-{run_id}"))
                .with_thread("same-parent-thread"),
        );
        (options, run)
    };
    let (options_a, parent_a) = build_parent(&workspace_a, &core_a);
    let (options_b, parent_b) = build_parent(&workspace_b, &core_b);
    let parent_cancel_a = options_a.run_context.cancellation.clone();
    let parent_cancel_b = options_b.run_context.cancellation.clone();
    // Control aliases run in a fresh turn context, as they do after a parent
    // turn resumes. Keep the original parents intact for their spawned child
    // futures; RunContext is intentionally not Clone.
    let mut run_a = options_a.run_context.clone().into_tinyagents(
        tinyagents_harness::context::RunConfig::new("workspace-a-control")
            .with_thread("same-parent-thread"),
    );
    let mut run_b = options_b.run_context.clone().into_tinyagents(
        tinyagents_harness::context::RunConfig::new("workspace-b-control")
            .with_thread("same-parent-thread"),
    );
    let jobs_a =
        crate::core::runtime::slot_for::<SubAgentJobRegistry>(&parent_a.data.dispatch_ctx());
    let jobs_b =
        crate::core::runtime::slot_for::<SubAgentJobRegistry>(&parent_b.data.dispatch_ctx());
    assert!(
        Arc::ptr_eq(&jobs_a, &jobs_b),
        "the two workspace overlays must exercise one shared registry"
    );
    let key_a = tinyagents_orchestration::subagent::SubagentTaskKey::from_context(
        &parent_a,
        "same-raw-task-id",
        Some("same-parent-thread".into()),
    );
    let key_b = tinyagents_orchestration::subagent::SubagentTaskKey::from_context(
        &parent_b,
        "same-raw-task-id",
        Some("same-parent-thread".into()),
    );

    let mut definition_a = AgentDefinitionRegistry::builtins_only()
        .get("task_manager_agent")
        .expect("task manager definition")
        .clone();
    definition_a.id = "workspace-a-worker".into();
    definition_a.model = tinyagents_definition::ModelSpec::Inherit;
    let mut definition_b = definition_a.clone();
    definition_b.id = "workspace-b-worker".into();
    let mut child_a = tokio::spawn(async move {
        run_subagent_via_driver_tool(
            &parent_a,
            definition_a,
            "run in workspace A".into(),
            options_a,
        )
        .await
    });
    let mut child_b = tokio::spawn(async move {
        run_subagent_via_driver_tool(
            &parent_b,
            definition_b,
            "run in workspace B".into(),
            options_b,
        )
        .await
    });
    let both_started = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for _ in 0..2 {
            tokio::select! {
                notification = started_rx.recv() => {
                    notification.expect("model start notification");
                }
                result = &mut child_a => match result {
                    Ok(Ok((outcome, output))) => panic!(
                        "workspace A child returned before entering the gated model: {:?}, {output}",
                        outcome.status,
                    ),
                    Ok(Err(error)) => panic!("workspace A child failed before model start: {error:?}"),
                    Err(error) => panic!("workspace A child task panicked before model start: {error}"),
                },
                result = &mut child_b => match result {
                    Ok(Ok((outcome, output))) => panic!(
                        "workspace B child returned before entering the gated model: {:?}, {output}",
                        outcome.status,
                    ),
                    Ok(Err(error)) => panic!("workspace B child failed before model start: {error:?}"),
                    Err(error) => panic!("workspace B child task panicked before model start: {error}"),
                },
            }
        }
    })
    .await;
    if both_started.is_err() {
        let jobs = jobs_a
            .list()
            .into_iter()
            .map(|job| format!("{}:{}:{:?}", job.id, job.agent, job.status))
            .collect::<Vec<_>>();
        panic!(
            "both driver children enter the gated model; jobs={jobs:?}, child_finished=({}, {})",
            child_a.is_finished(),
            child_b.is_finished()
        );
    }

    let job_a = jobs_a
        .list()
        .into_iter()
        .find(|job| job.agent == "workspace-a-worker")
        .expect("workspace A child is registered");
    let job_b = jobs_b
        .list()
        .into_iter()
        .find(|job| job.agent == "workspace-b-worker")
        .expect("workspace B child is registered");
    let id_a = job_a.id.as_str().to_owned();
    let id_b = job_b.id.as_str().to_owned();
    assert_ne!(
        id_a, id_b,
        "full lifecycle scope keeps raw-ID reuse distinct"
    );

    let now = chrono::Utc::now().to_rfc3339();
    let save_running_session =
        |workspace: &std::path::Path,
         session_id: &str,
         task_key: tinyagents_orchestration::subagent::SubagentTaskKey| {
            SubagentSessionStore::new(workspace.to_path_buf()).save(&[DurableSubagentSession {
                subagent_session_id: session_id.to_owned(),
                parent_session: "same-parent-session".into(),
                parent_thread_id: Some("same-parent-thread".into()),
                worker_thread_id: None,
                agent_id: "task_manager_agent".into(),
                display_name: None,
                model: Some("test-model".into()),
                sandbox_mode: "workspace".into(),
                action_root: None,
                task_key: "same task in another workspace".into(),
                task_title: "Workspace scoped task".into(),
                current_task_id: Some("same-raw-task-id".into()),
                lifecycle_key: Some(task_key),
                status: DurableSubagentStatus::Running,
                reusable: true,
                latest_history: None,
                latest_error: None,
                created_at: now.clone(),
                updated_at: now.clone(),
                last_used_at: now.clone(),
            }])
        };
    save_running_session(&workspace_a, "workspace-a-session", key_a)
        .expect("persist A session for native close alias");
    save_running_session(&workspace_b, "workspace-b-session", key_b)
        .expect("persist B session for native close alias");

    let tools: SharedToolSet = Arc::new(vec![
        Box::new(crate::agent::orchestration::tools::ListSubagentsTool::new()),
        Box::new(crate::agent::orchestration::tools::WaitSubagentTool::new()),
        Box::new(crate::agent::orchestration::tools::SteerSubagentTool::new()),
        Box::new(crate::agent::orchestration::tools::CloseSubagentTool::new()),
    ]);
    let sets = vec![tools];
    let mut harness: tinyagents_harness::runtime::AgentHarness<
        (),
        crate::agent::tinyagents::host::OpenHumanRunContext,
    > = tinyagents_harness::runtime::AgentHarness::new();
    crate::agent::tinyagents::tool_registration::host_tool_set_builder(
        &sets,
        &None,
        true,
        &std::collections::HashSet::new(),
    )
    .register(&mut harness, None);
    let mut status_a = tinyagents_harness::events::HarnessRunStatus::new(
        run_a.run_id().clone(),
        tinyagents_harness::ComponentId::new("workspace-a-control"),
    );
    let mut status_b = tinyagents_harness::events::HarnessRunStatus::new(
        run_b.run_id().clone(),
        tinyagents_harness::ComponentId::new("workspace-b-control"),
    );
    let mut agent_run_a = tinyagents_harness::middleware::AgentRun::new();
    let mut agent_run_b = tinyagents_harness::middleware::AgentRun::new();
    let mut messages_a = Vec::new();
    let mut messages_b = Vec::new();
    let listed_a = execute_tool_batch(
        &harness,
        &(),
        &mut run_a,
        &mut agent_run_a,
        &mut status_a,
        &mut messages_a,
        vec![tinyinference_llm::tool::ToolCall::new(
            "workspace-a-list",
            "list_subagents",
            serde_json::json!({}),
        )],
    )
    .await
    .expect("native list alias works for A");
    assert!(listed_a.results[0].text().contains("workspace-a-session"));
    assert!(
        !listed_a.results[0].text().contains("workspace-b-session"),
        "A must not see B's durable session: {}",
        listed_a.results[0].text()
    );
    let listed_b = execute_tool_batch(
        &harness,
        &(),
        &mut run_b,
        &mut agent_run_b,
        &mut status_b,
        &mut messages_b,
        vec![tinyinference_llm::tool::ToolCall::new(
            "workspace-b-list",
            "list_subagents",
            serde_json::json!({}),
        )],
    )
    .await
    .expect("native list alias works for B");
    assert!(listed_b.results[0].text().contains("workspace-b-session"));
    assert!(!listed_b.results[0].text().contains("workspace-a-session"));

    let foreign_wait = execute_tool_batch(
        &harness,
        &(),
        &mut run_a,
        &mut agent_run_a,
        &mut status_a,
        &mut messages_a,
        vec![tinyinference_llm::tool::ToolCall::new(
            "workspace-a-wait-b",
            "wait_subagent",
            serde_json::json!({"task_id":id_b,"timeout_secs":1}),
        )],
    )
    .await
    .expect("foreign wait returns a recoverable selector denial");
    assert_eq!(foreign_wait.results.len(), 1);
    let foreign_wait_text = foreign_wait.results[0].text();
    assert!(
        foreign_wait_text.contains("selected subagent job or session is unavailable"),
        "A must receive a selector denial for B's generic job id, got: {foreign_wait_text}"
    );
    assert_eq!(
        jobs_b
            .get(&id_b)
            .expect("B child remains registered")
            .status,
        tinyagents_orchestration::subagent::SubAgentJobStatus::Running,
        "a foreign wait must not settle or mutate B's child"
    );

    let foreign_close = execute_tool_batch(
        &harness,
        &(),
        &mut run_a,
        &mut agent_run_a,
        &mut status_a,
        &mut messages_a,
        vec![tinyinference_llm::tool::ToolCall::new(
            "workspace-a-close-b",
            "close_subagent",
            serde_json::json!({"subagent_session_id":"workspace-b-session"}),
        )],
    )
    .await
    .expect("foreign close is a recoverable model-facing selector error");
    assert!(!foreign_close.results[0].text().contains("\"closed\":true"));
    let job_b = jobs_b.get(&id_b).expect("B's child remains registered");
    assert_eq!(
        job_b.status,
        tinyagents_orchestration::subagent::SubAgentJobStatus::Running
    );

    let owned_close = execute_tool_batch(
        &harness,
        &(),
        &mut run_b,
        &mut agent_run_b,
        &mut status_b,
        &mut messages_b,
        vec![tinyinference_llm::tool::ToolCall::new(
            "workspace-b-close-own",
            "close_subagent",
            serde_json::json!({"subagent_session_id":"workspace-b-session"}),
        )],
    )
    .await
    .expect("B can close its own durable child");
    assert_eq!(owned_close.results.len(), 1);
    assert_eq!(
        SubagentSessionStore::new(workspace_b.clone())
            .load()
            .unwrap()[0]
            .status,
        DurableSubagentStatus::Closed
    );
    release.notify_waiters();
    let result_a = tokio::time::timeout(std::time::Duration::from_secs(5), child_a)
        .await
        .expect("A child settles")
        .expect("A task join")
        .expect("A child succeeds");
    let result_b = tokio::time::timeout(std::time::Duration::from_secs(5), child_b)
        .await
        .expect("B child settles")
        .expect("B task join");
    assert!(matches!(
        result_a.0.status,
        crate::agent::subagent_host::SubagentRunStatus::Completed
    ));
    match result_b {
        Ok((outcome, _)) => assert!(matches!(
            outcome.status,
            crate::agent::subagent_host::SubagentRunStatus::Cancelled
        )),
        Err(error) => assert_eq!(
            error.to_string(),
            "provider call failed: tinyagents harness run failed: run cancelled"
        ),
    }
    assert_eq!(
        jobs_b
            .get(&id_b)
            .expect("B driver job remains inspectable")
            .status,
        tinyagents_orchestration::subagent::SubAgentJobStatus::Cancelled,
        "the shared driver registry is authoritative for B's cancellation"
    );
    assert!(!parent_cancel_a.is_cancelled());
    assert!(!parent_cancel_b.is_cancelled());
}
