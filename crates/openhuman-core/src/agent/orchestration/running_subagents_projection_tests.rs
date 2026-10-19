use super::*;
use crate::core::runtime::{ContextOverlay, CoreContext, DispatchCtx, DomainSet, Mode};
use crate::tools::toolpacks::ToolGroups;
use tinyagents_orchestration::subagent::list_subagent_records;

const PARENT: &str = "projection-parent";

fn profile(name: &str, workspace: &std::path::Path) -> Arc<CoreContext> {
    CoreContext::for_test(DomainSet::full(), None).derive_with(
        ContextOverlay::new(
            crate::config::Config {
                workspace_dir: workspace.to_path_buf(),
                ..crate::config::Config::default()
            },
            DomainSet::kernel(),
            ToolGroups::none(),
        )
        .profile(name)
        .session_agent("projection-agent"),
    )
}

fn records(
    ctx: &DispatchCtx,
    workspace: &std::path::Path,
) -> Vec<tinyagents_tasks::OrchestrationTaskRecord> {
    let store = super::super::resolve::scoped_task_store(ctx, workspace)
        .expect("profile-scoped task store");
    list_subagent_records(store.as_ref())
}

#[tokio::test]
async fn explicit_profile_projects_spawn_completion_and_cancel_without_ambient_scope() {
    let name = "explicit_profile_projects_spawn_completion_and_cancel_without_ambient_scope";
    let module = module_path!().split_once("::").expect("crate module").1;
    let exact = format!("{module}::{name}");
    if std::env::var("OPENHUMAN_SUBAGENT_PROJECTION_CHILD").as_deref() != Ok(exact.as_str()) {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env("OPENHUMAN_SUBAGENT_PROJECTION_CHILD", &exact)
            .output()
            .expect("run isolated SaaS projection test");
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "isolated test {exact} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    crate::core::runtime::mode::lock_mode(Mode::Saas).expect("lock isolated SaaS mode");
    assert!(
        CoreContext::scoped().is_none(),
        "test starts without ambient scope"
    );
    let _guard = test_guard_async().await;
    let workspace = tempfile::tempdir().expect("workspace");
    let alice = DispatchCtx::new(profile("projection-alice", workspace.path()));
    let bob = DispatchCtx::new(profile("projection-bob", workspace.path()));
    let alice_task = uuid::Uuid::new_v4().to_string();
    let bob_task = uuid::Uuid::new_v4().to_string();

    let (alice_tx, alice_rx) = status_channel();
    let (bob_tx, bob_rx) = status_channel();
    register(
        &alice,
        alice_task.clone(),
        "researcher".into(),
        PARENT.into(),
        None,
        None,
        workspace.path().to_path_buf(),
        None,
        run_queue(),
        dummy_abort(),
        alice_rx,
    )
    .expect("Alice registration");
    CoreContext::sync_scope(alice.resolved().unwrap().clone(), || {
        register(
            &bob,
            bob_task.clone(),
            "researcher".into(),
            PARENT.into(),
            None,
            None,
            workspace.path().to_path_buf(),
            None,
            run_queue(),
            dummy_abort(),
            bob_rx,
        )
    })
    .expect("Bob registration under foreign ambient profile");

    let alice_spawned = records(&alice, workspace.path());
    let bob_spawned = records(&bob, workspace.path());
    assert!(alice_spawned
        .iter()
        .any(|row| row.spec.task_id.as_str() == alice_task));
    assert!(alice_spawned
        .iter()
        .all(|row| row.spec.task_id.as_str() != bob_task));
    assert!(bob_spawned
        .iter()
        .any(|row| row.spec.task_id.as_str() == bob_task));
    assert!(bob_spawned
        .iter()
        .all(|row| row.spec.task_id.as_str() != alice_task));

    alice_tx
        .send(DetachedSubagentStatus::Completed {
            output: "finished".into(),
            iterations: 1,
        })
        .expect("send terminal status");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if records(&alice, workspace.path())
                .iter()
                .any(|row| row.spec.task_id.as_str() == alice_task && row.is_terminal())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Alice terminal status projected");

    assert!(cancel_by_task(&alice, &bob_task).is_none());
    assert!(cancel_by_task(&bob, &bob_task).is_some());
    let alice_after_cancel = records(&alice, workspace.path());
    let bob_after_cancel = records(&bob, workspace.path());
    assert_eq!(
        alice_after_cancel
            .iter()
            .find(|row| row.spec.task_id.as_str() == alice_task)
            .expect("Alice row remains in Alice store")
            .status,
        OrchestrationTaskStatus::Completed
    );
    assert_eq!(
        bob_after_cancel
            .iter()
            .find(|row| row.spec.task_id.as_str() == bob_task)
            .expect("Bob row remains in Bob store")
            .status,
        OrchestrationTaskStatus::Cancelled
    );
    let _ = alice_tx;
    let _ = bob_tx;
}

#[tokio::test]
async fn cancellation_projection_uses_the_captured_profile_store() {
    let name = "cancellation_projection_uses_the_captured_profile_store";
    let module = module_path!().split_once("::").expect("crate module").1;
    let exact = format!("{module}::{name}");
    if std::env::var("OPENHUMAN_SUBAGENT_PROJECTION_CHILD").as_deref() != Ok(exact.as_str()) {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", &exact, "--nocapture", "--test-threads=1"])
            .env("OPENHUMAN_SUBAGENT_PROJECTION_CHILD", &exact)
            .output()
            .expect("run isolated SaaS cancellation test");
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "isolated test {exact} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    crate::core::runtime::mode::lock_mode(Mode::Saas).expect("lock isolated SaaS mode");
    let _guard = test_guard_async().await;
    let workspace = tempfile::tempdir().expect("workspace");
    let alice_context = profile("cancel-alice", workspace.path());
    let bob_context = profile("cancel-bob", workspace.path());
    let alice = DispatchCtx::new(alice_context.clone());
    let bob = DispatchCtx::new(bob_context.clone());
    let task_id = uuid::Uuid::new_v4().to_string();
    let (_status, receiver) = status_channel();
    CoreContext::sync_scope(alice_context, || {
        register(
            &alice,
            task_id.clone(),
            "researcher".into(),
            PARENT.into(),
            None,
            None,
            workspace.path().to_path_buf(),
            None,
            run_queue(),
            dummy_abort(),
            receiver,
        )
    })
    .expect("Alice registration");

    assert!(CoreContext::sync_scope(bob_context, || cancel_by_task(&alice, &task_id)).is_some());
    let alice_row = records(&alice, workspace.path())
        .into_iter()
        .find(|row| row.spec.task_id.as_str() == task_id)
        .expect("Alice cancellation is persisted to Alice's profile store");
    assert_eq!(alice_row.status, OrchestrationTaskStatus::Cancelled);
    assert!(records(&bob, workspace.path())
        .iter()
        .all(|row| row.spec.task_id.as_str() != task_id));
}
