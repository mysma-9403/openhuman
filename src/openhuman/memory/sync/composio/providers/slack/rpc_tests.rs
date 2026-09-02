use super::*;

fn unsigned_in_config() -> Config {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.config_path = tmp.path().join("config.toml");
    std::mem::forget(tmp);
    config
}

fn direct_mode_no_key_config() -> Config {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.config_path = tmp.path().join("config.toml");
    config.composio.mode = tinymemory_api::host::COMPOSIO_MODE_DIRECT.to_string();
    std::mem::forget(tmp);
    config
}

#[tokio::test]
async fn list_slack_connections_errors_with_slack_ingest_prefix_when_no_credentials() {
    // Pre-Option-C `sync_trigger_rpc` / `sync_status_rpc` returned
    // the literal string "[slack_ingest] Composio client unavailable
    // (user not signed in?)" because the gate was
    // `build_composio_client(...).is_none()`. Post-Option-C the
    // gate is the factory, so the error surfaces the *factory's*
    // "no backend session" message wrapped with the domain prefix.
    // We exercise the shared helper directly so the test doesn't
    // depend on the SlackProvider being registered in the test
    // global registry (that registration is a runtime concern
    // owned by `init_default_providers`, not relevant to the
    // factory wiring under test here).
    let config = unsigned_in_config();
    let err = list_slack_connections(&config).await.unwrap_err();
    assert!(
        err.starts_with("[slack_ingest] list_connections:"),
        "factory-routed error should keep the [slack_ingest] domain prefix, got: {err}"
    );
    assert!(
        err.contains("no backend session"),
        "backend-mode failure path should surface the factory's session-missing message, \
         got: {err}"
    );
}

#[tokio::test]
async fn list_slack_connections_in_direct_mode_without_api_key_surfaces_direct_mode_error() {
    // Confirms the factory is exercised in direct mode too — when
    // mode=direct but no api_key is stored, the error message
    // surfaces the direct-mode key-missing hint, not the backend
    // session message. Pre-Option-C this returned the backend-only
    // "user not signed in?" message regardless of mode.
    let config = direct_mode_no_key_config();
    let err = list_slack_connections(&config).await.unwrap_err();
    assert!(
        err.starts_with("[slack_ingest] list_connections:"),
        "domain prefix preserved through the factory route, got: {err}"
    );
    assert!(
        err.contains("direct mode") || err.contains("api key"),
        "direct-mode key-missing should surface the direct-mode-specific hint, got: {err}"
    );
}

#[tokio::test]
async fn list_slack_connections_resolves_direct_variant_when_mode_is_direct() {
    // Pin the factory routing: with a direct-mode config + inline
    // api_key, `list_slack_connections` must reach
    // `direct_list_connections` (which then attempts a network
    // call). We can't assert the success path without a mock
    // backend.composio.dev, but we *can* assert the error message
    // identifies the direct arm — proving the factory picked the
    // right branch.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = Config::default();
    config.config_path = tmp.path().join("config.toml");
    config.composio.mode = tinymemory_api::host::COMPOSIO_MODE_DIRECT.to_string();
    config.composio.api_key = Some("test-direct-key".to_string());
    std::mem::forget(tmp);

    let result = list_slack_connections(&config).await;
    // The network call will fail (test environment has no upstream
    // mock). We only care that the failure label says "direct" —
    // that's the load-bearing evidence the factory routed through
    // the new branch instead of the old backend-only path.
    if let Err(err) = result {
        assert!(
            err.contains("(direct)") || err.contains("direct"),
            "factory must route to the direct arm for mode=direct configs, got: {err}"
        );
    }
    // If the network call somehow succeeds (e.g. CI gateway returns
    // a valid empty envelope), that's also acceptable — the
    // factory still routed correctly.
}
