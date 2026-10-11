use super::*;
use crate::core::runtime::{context::CoreContext, ContextOverlay, DomainSet};
use crate::tools::toolpacks::ToolGroups;

fn workspace_config(workspace: &Path) -> Config {
    let mut config = Config::default();
    config.gitbooks.enabled = false;
    config.workspace_dir = workspace.to_path_buf();
    config
}

fn agent_context(config: &Config, agent: &str) -> Arc<CoreContext> {
    CoreContext::for_test_with_config(DomainSet::full(), config.clone()).derive_with(
        ContextOverlay::new(config.clone(), DomainSet::full(), ToolGroups::none())
            .session_agent(agent),
    )
}

#[tokio::test]
async fn each_agent_gets_its_own_host_under_its_scope_dir() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let config = workspace_config(temporary.path());

    let alpha = CoreContext::scope(agent_context(&config, "alpha"), async {
        for_config(&config).expect("alpha's host opens")
    })
    .await;
    let beta = CoreContext::scope(agent_context(&config, "beta"), async {
        for_config(&config).expect("beta's host opens")
    })
    .await;
    let workspace = for_config(&config).expect("the workspace host opens");

    assert!(!Arc::ptr_eq(&alpha, &beta));
    assert!(!Arc::ptr_eq(&alpha, &workspace));
    let alpha_dir = temporary.path().join("agents").join("alpha");
    assert!(tinymcp::Store::path_for(&alpha_dir).exists());
    assert!(tinymcp::AuditStore::path_for(&alpha_dir).exists());
    assert!(tinymcp::Store::path_for(temporary.path()).exists());
}

#[tokio::test]
async fn the_ambient_service_under_an_agent_is_that_agents_host() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let config = workspace_config(temporary.path());

    let (addressed, ambient) = CoreContext::scope(agent_context(&config, "gamma"), async {
        let addressed = for_config(&config).expect("the agent's host opens");
        (
            addressed,
            try_service().expect("an opened agent host is ambient"),
        )
    })
    .await;

    assert!(Arc::ptr_eq(&ambient, &addressed));
    let outside = for_config(&config).expect("the workspace host opens");
    assert!(!Arc::ptr_eq(&ambient, &outside));
}

#[tokio::test]
async fn an_evicted_agent_host_is_reopened_fresh() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let config = workspace_config(temporary.path());

    let first = CoreContext::scope(agent_context(&config, "delta"), async {
        for_config(&config).expect("delta's host opens")
    })
    .await;
    let taken = take_agent_host(temporary.path(), "delta").expect("delta had a host");
    assert!(Arc::ptr_eq(&first, &taken));
    assert!(take_agent_host(temporary.path(), "delta").is_none());

    let reopened = CoreContext::scope(agent_context(&config, "delta"), async {
        for_config(&config).expect("delta's host reopens")
    })
    .await;
    assert!(!Arc::ptr_eq(&first, &reopened));
}

#[tokio::test]
async fn reading_an_agents_connections_does_not_create_its_host() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let config = workspace_config(temporary.path());

    let (looked_up, ambient) = CoreContext::scope(agent_context(&config, "epsilon"), async {
        (lookup(&config).is_err(), try_service().is_none())
    })
    .await;

    assert!(looked_up, "no host exists for a fresh agent");
    assert!(ambient, "the ambient lookup answers nothing connected");
    assert!(!temporary.path().join("agents").join("epsilon").exists());
}

/// A profile's context as the SaaS profile host derives it: the profile's
/// own workspace, a profile id, and no agent id.
fn profile_context(config: &Config, profile: &str) -> Arc<CoreContext> {
    CoreContext::for_test_with_config(DomainSet::full(), config.clone()).derive_with(
        ContextOverlay::new(config.clone(), DomainSet::full(), ToolGroups::none()).profile(profile),
    )
}

#[tokio::test]
async fn two_profiles_default_agents_get_their_own_hosts_and_never_the_default() {
    let (one, two) = (
        tempfile::tempdir().expect("tempdir"),
        tempfile::tempdir().expect("tempdir"),
    );
    let (alice, bob) = (workspace_config(one.path()), workspace_config(two.path()));

    // Before either installed anything, neither sees a host — not each
    // other's, not the process default — and a read creates none.
    for (config, name) in [(&alice, "alice"), (&bob, "bob")] {
        let seen = CoreContext::scope(profile_context(config, name), async { try_service() }).await;
        assert!(seen.is_none(), "{name} saw a host it never opened");
        assert!(!tinymcp::Store::path_for(&config.workspace_dir).exists());
    }

    let alice_host = CoreContext::scope(profile_context(&alice, "alice"), async {
        for_config(&alice).expect("alice's host opens")
    })
    .await;
    let bob_host = CoreContext::scope(profile_context(&bob, "bob"), async {
        for_config(&bob).expect("bob's host opens")
    })
    .await;
    assert!(!Arc::ptr_eq(&alice_host, &bob_host));
    // The default agent's host sits at the profile's workspace root.
    assert!(tinymcp::Store::path_for(one.path()).exists());
    assert!(!one.path().join("agents").exists());

    let ambient_alice =
        CoreContext::scope(profile_context(&alice, "alice"), async { try_service() }).await;
    let ambient_bob =
        CoreContext::scope(profile_context(&bob, "bob"), async { try_service() }).await;
    assert!(Arc::ptr_eq(
        &ambient_alice.expect("alice's host"),
        &alice_host
    ));
    assert!(Arc::ptr_eq(&ambient_bob.expect("bob's host"), &bob_host));
}
