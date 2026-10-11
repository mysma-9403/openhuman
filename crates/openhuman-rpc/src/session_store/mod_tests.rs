use super::*;
use std::sync::Mutex;
use tinyagents_session::testkit::conformance::session_store_conformance;
use tinyagents_session::turn_state::{TurnLifecycle, TurnState};

#[tokio::test]
async fn the_desktop_layout_meets_the_session_store_contract() {
    let dir = tempfile::tempdir().unwrap();
    session_store_conformance(&SqliteSessionStores::at(dir.path())).await;
    // Everything landed in the classic layout.
    assert!(dir.path().join("session_raw").is_dir());
    assert!(dir.path().join("tinyagents_store").is_dir());
}

#[test]
fn the_workspace_follows_the_resolver() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let current = Arc::new(Mutex::new(first.path().to_path_buf()));
    let resolver = current.clone();
    let stores = SqliteSessionStores::resolving(move || resolver.lock().unwrap().clone());
    assert_eq!(stores.workspace_dir().as_deref(), Some(first.path()));
    *current.lock().unwrap() = second.path().to_path_buf();
    assert_eq!(stores.workspace_dir().as_deref(), Some(second.path()));
    assert_eq!(
        stores.destination_key(),
        Some(second.path().to_string_lossy().into_owned())
    );
    assert!(format!("{stores:?}").contains("SqliteSessionStores"));
}

#[test]
fn recovery_interrupts_turns_left_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let stores = SqliteSessionStores::at(dir.path());
    let agent = stores.for_agent("orchestrator");
    agent
        .turn_states
        .put(&TurnState::started("t", "r", 4, "2026-01-01T00:00:00Z"))
        .unwrap();
    stores.recover().unwrap();
    assert_eq!(
        agent
            .turn_states
            .get("t")
            .unwrap()
            .map(|turn| turn.lifecycle),
        Some(TurnLifecycle::Interrupted)
    );
}

/// The provider and storage slots are process-wide; these tests take turns
/// with each other and with any test that stores a credential.
use crate::STORAGE_SLOT_TEST_LOCK as SLOTS;

/// Puts the process-global storage backend back as a test found it.
fn restore_backend(previous: Option<Arc<dyn crate::core_host::storage::StorageBackend>>) {
    match previous {
        Some(backend) => {
            crate::core_host::storage::install(backend);
        }
        None => {
            crate::core_host::storage::clear();
        }
    }
}

#[tokio::test]
async fn a_storage_url_installs_the_driver_backed_store() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    install_for_url(Some("memory".into())).await.unwrap();

    let provider = crate::core_host::agent::session_store::installed().unwrap();
    assert!(
        provider
            .destination_key()
            .is_some_and(|key| key.starts_with("memory://")),
        "{:?}",
        provider.destination_key()
    );
    assert_eq!(
        crate::core_host::storage::installed().map(|b| b.driver()),
        Some("memory")
    );
    // Agents are kept apart in the shared backend.
    let alice = provider.for_agent("alice");
    alice
        .turn_states
        .put(&TurnState::started("t", "r", 8, "2026-01-01T00:00:00Z"))
        .unwrap();
    assert!(provider
        .for_agent("bob")
        .turn_states
        .get("t")
        .unwrap()
        .is_none());

    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);
}

/// A storage URL installs the driver-backed store, and two agents on the one
/// backend keep their turn states apart. Run once per driver the build has.
async fn driver_backed_store_keeps_agents_apart(url: String, driver: &str) {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    install_for_url(Some(url)).await.unwrap();

    assert_eq!(
        crate::core_host::storage::installed().map(|b| b.driver()),
        Some(driver)
    );
    let provider = crate::core_host::agent::session_store::installed().unwrap();
    let alice = provider.for_agent("alice");
    alice
        .turn_states
        .put(&TurnState::started("t", "r", 8, "2026-01-01T00:00:00Z"))
        .unwrap();
    assert!(alice.turn_states.get("t").unwrap().is_some());
    assert!(provider
        .for_agent("bob")
        .turn_states
        .get("t")
        .unwrap()
        .is_none());

    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test(flavor = "multi_thread")]
async fn the_sqlite_backend_keeps_agents_apart() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("sessions").display());
    driver_backed_store_keeps_agents_apart(url, "sqlite").await;
}

#[cfg(feature = "storage-file")]
#[tokio::test(flavor = "multi_thread")]
async fn the_file_backend_keeps_agents_apart() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("file:{}", dir.path().join("files").display());
    driver_backed_store_keeps_agents_apart(url, "file").await;
}

/// Whether every seed host of a MongoDB URL is exactly the local machine
/// (`localhost`, `127.0.0.1` or `::1`); a prefix match would accept
/// `127.0.0.1.example.com`. Same rule as the root suites' helper
/// (`tests/support/storage_drivers.rs`).
#[cfg(feature = "storage-mongodb")]
fn is_loopback_mongo_url(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    let hosts = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    !hosts.is_empty()
        && !url.starts_with("mongodb+srv://")
        && hosts.split(',').all(|host| {
            let name = if let Some(v6) = host.strip_prefix('[') {
                v6.split(']').next().unwrap_or_default()
            } else {
                host.rsplit_once(':').map_or(host, |(name, _)| name)
            };
            matches!(name, "localhost" | "127.0.0.1" | "::1")
        })
}

/// Runs against `TSD_MONGO_URL` (a throwaway local replica set, as the
/// `Storage (MongoDB)` workflow starts) in a database of its own; skipped
/// when it is unset or names a host other than this machine.
#[cfg(feature = "storage-mongodb")]
#[tokio::test(flavor = "multi_thread")]
async fn the_mongodb_backend_keeps_agents_apart() {
    let Some(base) = std::env::var("TSD_MONGO_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        eprintln!("skipped mongodb: TSD_MONGO_URL is not set");
        return;
    };
    if !is_loopback_mongo_url(&base) {
        eprintln!("skipped mongodb: TSD_MONGO_URL must name a local server");
        return;
    }
    let (head, query) = match base.split_once('?') {
        Some((head, query)) => (head, format!("?{query}")),
        None => (base.as_str(), String::new()),
    };
    let authority_end = head[head.find("://").map_or(0, |at| at + 3)..]
        .find('/')
        .map_or(head.len(), |at| head.find("://").map_or(0, |i| i + 3) + at);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let url = format!(
        "{}/oh_rpc_{nanos:x}_{}{query}",
        &head[..authority_end],
        std::process::id()
    );
    driver_backed_store_keeps_agents_apart(url, "mongodb").await;
}

#[tokio::test]
async fn no_url_keeps_the_classic_layout() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    install_for_url(None).await.unwrap();
    let provider = crate::core_host::agent::session_store::installed().unwrap();
    assert!(
        provider.workspace_dir().is_some(),
        "the file layout is installed"
    );
    assert!(crate::core_host::storage::installed().is_none());
    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);
}

#[tokio::test]
async fn an_unusable_url_fails_the_boot() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let error = install_for_url(Some("ftp://nowhere".into()))
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("storage"), "{error:#}");
    crate::core_host::agent::session_store::restore(previous);
}

#[tokio::test]
async fn restoring_the_classic_layout_clears_a_previous_backend() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    install_for_url(Some("memory".into())).await.unwrap();
    assert!(crate::core_host::storage::installed().is_some());
    install_for_url(None).await.unwrap();
    assert!(crate::core_host::storage::installed().is_none());
    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);
}

#[tokio::test]
async fn the_host_reads_the_storage_url_from_the_environment() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    let var = crate::core_host::storage::STORAGE_URL_VAR;
    let old = std::env::var_os(var);

    std::env::set_var(var, " memory ");
    let booted = install_for_host().await;
    let driver = crate::core_host::storage::installed().map(|b| b.driver());

    std::env::set_var(var, "ftp://nowhere");
    let refused = install_for_host().await;

    match old {
        Some(value) => std::env::set_var(var, value),
        None => std::env::remove_var(var),
    }
    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);

    booted.unwrap();
    assert_eq!(driver, Some("memory"));
    assert!(refused.is_err(), "an unusable env URL fails the boot");
}

#[tokio::test]
async fn provider_for_url_returns_the_store_without_installing_it() {
    let _turn = SLOTS.lock().await;
    let previous = crate::core_host::agent::session_store::installed();
    let previous_backend = crate::core_host::storage::installed();
    crate::core_host::agent::session_store::restore(None);

    let backed = provider_for_url(Some("memory".into())).await.unwrap();
    let backed_key = backed.destination_key();
    let backend_driver = crate::core_host::storage::installed().map(|b| b.driver());
    let left_uninstalled = crate::core_host::agent::session_store::installed().is_none();

    let classic = provider_for_url(None).await.unwrap();
    let cleared = crate::core_host::storage::installed().is_none();

    restore_backend(previous_backend);
    crate::core_host::agent::session_store::restore(previous);

    assert!(
        backed_key
            .as_deref()
            .is_some_and(|key| key.starts_with("memory://")),
        "{backed_key:?}"
    );
    assert_eq!(backend_driver, Some("memory"), "the backend is installed");
    assert!(
        left_uninstalled,
        "the provider is handed back, not installed"
    );
    assert!(
        classic.workspace_dir().is_some(),
        "no URL is the file layout"
    );
    assert!(cleared, "no URL clears an earlier backend");
}

#[tokio::test]
async fn provider_for_host_reads_the_storage_url_from_the_environment() {
    let _turn = SLOTS.lock().await;
    let previous_backend = crate::core_host::storage::installed();
    let var = crate::core_host::storage::STORAGE_URL_VAR;
    let old = std::env::var_os(var);

    std::env::set_var(var, "memory");
    let booted = provider_for_host().await.map(|p| p.destination_key());
    std::env::set_var(var, "ftp://nowhere");
    let refused = provider_for_host().await;

    match old {
        Some(value) => std::env::set_var(var, value),
        None => std::env::remove_var(var),
    }
    restore_backend(previous_backend);

    let key = booted.unwrap();
    assert!(
        key.as_deref()
            .is_some_and(|key| key.starts_with("memory://")),
        "{key:?}"
    );
    assert!(refused.is_err(), "an unusable env URL fails the TUI boot");
}
