//! Git-worktree isolation for parallel, edit-capable agent workers.
//!
//! Parallel coding workers spawned via [`super::tools::spawn_parallel_agents`]
//! historically shared one workspace (`Config.action_dir`). Two workers editing
//! overlapping files left the parent with stale assumptions and silent
//! clobbers. Each isolated worker instead gets its own `git worktree` checkout
//! of the **user's project repo** (the directory the coding agent edits), so
//! file edits never collide.
//!
//! ## Where the implementation lives
//!
//! The git plumbing itself is **TinyAgents'**
//! ([`tinyagents_harness::workspace::git`]): worktree create/list/status/diff/
//! remove, repo-root validation, run-id sanitizing, and cross-worker overlap
//! detection. Callers import that surface directly from the harness.
//!
//! What stays in OpenHuman is the part that depends on *this* host: the
//! [`OpenHumanWorktreeIsolation`] adapter, which stamps an
//! `openhuman.worktree:{agent}:{run_id}` policy id onto prepared descriptors and
//! announces workspace lifecycle on the OpenHuman event bus so audit and
//! observability subscribers can correlate an isolated run with its allowed
//! root.
//!
//! ## Scope and safety
//!
//! - This targets the **user's project repository** rooted at the agent's
//!   `action_dir` — it never operates on OpenHuman's own source tree.
//! - Every operation validates that `repo_root` is a real git repository
//!   first (via `git rev-parse --is-inside-work-tree`), so a stray path can
//!   never be mutated.
//! - `remove_git_worktree` refuses to delete a **dirty** worktree unless
//!   `force = true`.
//!   Clean worktrees can be auto-reclaimed; dirty ones require an explicit
//!   user decision (acceptance criterion of #3376).
//!
//! The wrapper shells out to `git` through [`std::process::Command`] with an
//! explicit, validated working directory. It does not inherit ambient git
//! configuration that could redirect operations elsewhere.

use std::path::PathBuf;

use tinyagents_harness::workspace::{
    GitWorktreeBaseRef, GitWorktreeIsolation, GitWorktreeLifecycleHook, WorkspaceIsolation,
};
use tinytools::{SandboxMode, WorkspaceDescriptor};

use crate::core::bus::BUS;
use crate::core::events::DomainEvent;

/// OpenHuman's [`WorkspaceIsolation`] adapter over the TinyAgents git-worktree
/// provider.
///
/// The crate's [`GitWorktreeIsolation`] already prepares and cleans up the
/// checkout. This wrapper adds the two things that are OpenHuman's rather than
/// every host's: the `openhuman.worktree:*` policy-id convention, and
/// [`DomainEvent::WorkspacePrepared`] / [`DomainEvent::WorkspaceCleanup`]
/// announcements on the global bus.
#[derive(Debug, Clone)]
pub struct OpenHumanWorktreeIsolation {
    inner: GitWorktreeIsolation,
}

impl OpenHumanWorktreeIsolation {
    /// Create an isolation provider rooted at the user's project repo.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            inner: GitWorktreeIsolation::new(repo_root)
                .with_policy_id_prefix("openhuman.worktree")
                .with_lifecycle_hook(std::sync::Arc::new(OpenHumanWorktreeLifecycle)),
        }
    }

    /// Select which ref new worktrees branch from.
    pub fn with_base_ref(mut self, base_ref: GitWorktreeBaseRef) -> Self {
        self.inner = self.inner.with_base_ref(base_ref);
        self
    }

    /// Advertise the sandbox expectation on prepared descriptors.
    pub fn with_sandbox(mut self, sandbox: SandboxMode) -> Self {
        self.inner = self.inner.with_sandbox(sandbox);
        self
    }

    /// Add an extra root tools may touch alongside the isolated checkout.
    pub fn with_trusted_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.inner = self.inner.with_trusted_root(root);
        self
    }
}

struct OpenHumanWorktreeLifecycle;

#[async_trait::async_trait]
impl GitWorktreeLifecycleHook for OpenHumanWorktreeLifecycle {
    async fn prepared(&self, descriptor: &WorkspaceDescriptor) -> tinyagents_harness::Result<()> {
        let _ = BUS.publish(DomainEvent::WorkspacePrepared {
            policy_id: descriptor.policy_id.clone(),
            root: descriptor.root.display().to_string(),
        });
        Ok(())
    }

    async fn cleaned(
        &self,
        descriptor: &WorkspaceDescriptor,
        error: Option<&str>,
    ) -> tinyagents_harness::Result<()> {
        let _ = BUS.publish(DomainEvent::WorkspaceCleanup {
            policy_id: descriptor.policy_id.clone(),
            error: error.map(str::to_owned),
        });
        Ok(())
    }
}

#[async_trait::async_trait]
impl WorkspaceIsolation for OpenHumanWorktreeIsolation {
    async fn prepare(
        &self,
        run_id: &str,
        agent: Option<&str>,
    ) -> tinyagents_harness::Result<WorkspaceDescriptor> {
        tracing::debug!(
            run_id,
            agent = agent.unwrap_or(""),
            "[worktree] workspace_prepare_start"
        );

        let descriptor = self.inner.prepare(run_id, agent).await?;

        tracing::debug!(
            root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[worktree] workspace_prepare_done"
        );
        // Announce the prepared workspace so audit/observability subscribers can
        // correlate the isolated run with its allowed root.
        tracing::debug!(
            root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[workspace] workspace_prepared_emit"
        );
        Ok(descriptor)
    }

    async fn cleanup(&self, descriptor: &WorkspaceDescriptor) -> tinyagents_harness::Result<()> {
        tracing::debug!(
            root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[worktree] workspace_cleanup_start"
        );

        match self.inner.cleanup(descriptor).await {
            Ok(()) => {
                tracing::debug!(
                    root = %descriptor.root.display(),
                    policy_id = %descriptor.policy_id,
                    "[worktree] workspace_cleanup_done"
                );
                tracing::debug!(
                    policy_id = %descriptor.policy_id,
                    "[workspace] workspace_cleanup_emit_ok"
                );
                Ok(())
            }
            Err(err) => {
                let message = err.to_string();
                tracing::warn!(
                    root = %descriptor.root.display(),
                    policy_id = %descriptor.policy_id,
                    error = %message,
                    "[worktree] workspace_cleanup_failed"
                );
                tracing::debug!(
                    policy_id = %descriptor.policy_id,
                    error = %message,
                    "[workspace] workspace_cleanup_emit_err"
                );
                Err(err)
            }
        }
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
