use super::*;
use tinyagents_tasks::InMemoryCompletionStore;

/// A store whose writes always fail, like a full disk.
struct FullDisk(InMemoryCompletionStore);

impl CompletionStore for FullDisk {
    fn get(&self, task_id: &str) -> Option<CompletionRecord> {
        self.0.get(task_id)
    }
    fn put(&self, _record: &CompletionRecord) -> tinyagents_harness::error::Result<()> {
        Err(tinyagents_harness::error::TinyAgentsError::Graph(
            "disk full".into(),
        ))
    }
    fn list(&self, parent_key: Option<&str>) -> Vec<CompletionRecord> {
        self.0.list(parent_key)
    }
}

#[derive(Default)]
struct ToggleStore {
    inner: InMemoryCompletionStore,
    fail_writes: AtomicBool,
}

impl CompletionStore for ToggleStore {
    fn get(&self, task_id: &str) -> Option<CompletionRecord> {
        self.inner.get(task_id)
    }

    fn put(&self, record: &CompletionRecord) -> tinyagents_harness::error::Result<()> {
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(tinyagents_harness::error::TinyAgentsError::Graph(
                "disk full".into(),
            ));
        }
        self.inner.put(record)
    }

    fn list(&self, parent_key: Option<&str>) -> Vec<CompletionRecord> {
        self.inner.list(parent_key)
    }
}

#[tokio::test]
async fn a_store_that_keeps_failing_degrades_to_memory_instead_of_losing_the_result() {
    let _guard = crate::config::TEST_ENV_LOCK.lock().await;
    let ws = workspace();
    let w = ws.path();
    install_store_for_test(w, Arc::new(FullDisk(InMemoryCompletionStore::new())));

    record_completion(
        &cx(),
        w,
        "sess-fd",
        "sub-1",
        "researcher",
        "must not be lost",
        Some("thread-fd".into()),
    )
    .await;

    let pending = pending_for(w, "thread-fd");
    assert_eq!(pending.len(), 1, "the result is held in memory");
    assert_eq!(pending[0].result.text, "must not be lost");
}

#[tokio::test]
async fn reopening_a_durable_store_preserves_pending_memory_fallback_records() {
    let _guard = crate::config::TEST_ENV_LOCK.lock().await;
    let ws = workspace();
    let w = ws.path();
    install_store_for_test(w, Arc::new(FullDisk(InMemoryCompletionStore::new())));

    record_completion(
        &cx(),
        w,
        "sess-reopen",
        "memory-task",
        "researcher",
        "held in memory",
        Some("thread-reopen".into()),
    )
    .await;

    let before = pending_for(w, "thread-reopen");
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].result.text, "held in memory");
    drop(before);

    let ctx = crate::core::runtime::DispatchCtx::ambient();
    forget_recovery_ctx(&ctx, w);

    let after = pending_for(w, "thread-reopen");
    assert_eq!(
        after.len(),
        1,
        "re-opening must migrate memory-held records"
    );
    assert_eq!(after[0].result.text, "held in memory");
}

#[tokio::test]
async fn concurrent_degradation_installs_one_fallback() {
    let _guard = crate::config::TEST_ENV_LOCK.lock().await;
    let ws = workspace();
    let w = ws.path();
    let failing = router_for_workspace(w);

    // Two records saw the same failing router; both ask to degrade.
    let ctx = crate::core::runtime::DispatchCtx::ambient();
    let first = degrade_to_memory(&ctx, w, &failing);
    let second = degrade_to_memory(&ctx, w, &failing);

    assert!(
        Arc::ptr_eq(&first.router, &second.router),
        "the second caller reuses the fallback the first installed"
    );
    assert!(Arc::ptr_eq(&entry_for(w).router, &first.router));
}

#[tokio::test]
async fn degrading_a_leased_completion_keeps_its_lease() {
    let _guard = crate::config::TEST_ENV_LOCK.lock().await;
    let ws = workspace();
    let w = ws.path();
    let ctx = crate::core::runtime::DispatchCtx::ambient();
    let store = Arc::new(ToggleStore::default());
    install_store_for_test(w, store.clone());
    let router = router_for_workspace(w);
    router
        .record(CompletionRecord::new(
            "leased-task",
            "leased-thread",
            "researcher",
            CompletionStatus::Success,
            CompletionResult::text("done"),
        ))
        .await
        .expect("record completion");
    assert_eq!(router.claim_pending("leased-thread", 1).unwrap().len(), 1);
    store.fail_writes.store(true, Ordering::SeqCst);

    record_completion(
        &ctx,
        w,
        "leased-session",
        "trigger-task",
        "researcher",
        "force fallback",
        Some("leased-thread".into()),
    )
    .await;

    let claims = router_for_workspace(w)
        .claim_pending("leased-thread", 10)
        .unwrap();
    assert!(
        claims.iter().all(|record| record.task_id != "leased-task"),
        "a fallback router must not make another delivery claim while the first lease is active"
    );
}
