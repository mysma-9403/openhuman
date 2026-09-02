use super::*;

/// Privacy epic S7 (#4441): under LocalOnly, `embed` refuses before touching
/// the inner cloud transport (so no network / no auth is needed to observe
/// the block). Uses the thread-scoped privacy override so it never mutates
/// the process-global policy that sibling tests read.
#[tokio::test]
async fn embed_blocked_under_local_only() {
    let _mode = crate::openhuman::security::live_policy::test_privacy_scope(
        crate::openhuman::config::PrivacyMode::LocalOnly,
    );
    let provider = OpenHumanCloudEmbedding::new(
        Some("http://127.0.0.1:0".into()),
        Some(std::env::temp_dir().join("openhuman_embeddings_localonly_state")),
        false,
        DEFAULT_CLOUD_EMBEDDING_MODEL,
        DEFAULT_CLOUD_EMBEDDING_DIMENSIONS,
    );

    let err = provider.embed(&["hello"]).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("Local-only privacy mode is active"),
        "unexpected error: {err}"
    );
}

/// The keyless credential scope must land in the **user-scoped** directory,
/// never the root. Sign-in writes `auth-profiles.json` to
/// `{root}/users/<user_id>/`; the root itself holds no such file, so the
/// previous root-returning implementation made every keyless managed
/// embedder fail with "No backend session for cloud embeddings" for a user
/// who was signed in.
#[test]
fn default_scope_is_the_active_user_dir_not_the_root() {
    let root = std::path::Path::new("/tmp/openhuman-root");

    let resolved = user_scoped_state_dir(root, Some("user-abc123"));

    assert_eq!(
        resolved,
        root.join("users").join("user-abc123"),
        "managed embedder must read credentials from the active user's dir"
    );
    assert_ne!(
        resolved, root,
        "the root dir holds no auth-profiles.json — resolving to it is the bug"
    );
}

/// With no user signed in yet, the scope is the pre-login user dir — the
/// same directory the pre-login config and its credential store live in.
/// Falling back to the root here would reintroduce the same empty-store
/// failure one login earlier.
#[test]
fn default_scope_falls_back_to_the_pre_login_user_dir() {
    let root = std::path::Path::new("/tmp/openhuman-root");

    let resolved = user_scoped_state_dir(root, None);

    assert_eq!(
        resolved,
        root.join("users")
            .join(crate::openhuman::config::PRE_LOGIN_USER_ID),
        "a pre-login process must read its own store, not the empty root"
    );
}

/// `OPENHUMAN_WORKSPACE` must resolve through the same workspace→config-dir
/// mapping `config::load` uses, not return the raw workspace path. A legacy
/// `<X>/workspace` override keeps its credentials in the sibling
/// `<X>/.openhuman` dir; returning the workspace dir itself would send the
/// keyless embedder to a directory with no `auth-profiles.json` and
/// reintroduce "No backend session" for that deployment.
#[test]
fn env_workspace_scope_is_the_config_dir_not_the_raw_workspace() {
    // A path that does not exist on disk, so the resolver's `config.toml`
    // probes both miss and the `"workspace"` basename rule decides.
    let workspace = std::path::Path::new("/nonexistent-openhuman-test-root/workspace");

    let resolved = env_workspace_state_dir(workspace);

    assert_eq!(
        resolved,
        std::path::Path::new("/nonexistent-openhuman-test-root/.openhuman"),
        "a `.../workspace` override must resolve to its sibling .openhuman config dir"
    );
    assert_ne!(
        resolved, workspace,
        "returning the raw workspace dir is the regression this guards against"
    );
}
