//! Shared driver matrix for the `storage_*_e2e` suites and
//! `cli_storage_url_e2e`.
//!
//! Every suite that exercises the core on a configured storage backend runs
//! the same body once per driver, each as its own test case named after the
//! driver (`memory::...`, `sqlite::...`, `file::...`, `mongodb::...`), so a
//! failure says which driver broke:
//!
//! - `memory`: always;
//! - `sqlite`: when the `storage-sqlite` feature is compiled in, on a temp
//!   directory (the desktop default, one `.db` file per database);
//! - `file`: when the `storage-file` feature is compiled in, on a temp
//!   directory;
//! - `mongodb`: when `storage-mongodb` is compiled in AND `TSD_MONGO_URL` is
//!   set to a local throwaway server (localhost, 127.0.0.1 or ::1; a replica set, as `tinystoragedrivers`' CI uses). Each case gets a
//!   database of its own (`oh_e2e_<uuid>`), so runs never see each other's
//!   records and scopes stay isolated. With `TSD_MONGO_URL` unset the case
//!   passes after printing that it was skipped.
//!
//! The backend slot is process-wide, so the cases of one binary run one at a
//! time (they share [`Case`]'s lock) and the slot is emptied when a case ends,
//! pass or fail.
//!
//! Include with `#[macro_use] mod storage_drivers;` and declare the body with
//! `driver_cases!(sync body)` (or `async body`), where `body` is
//! `fn(Case)` (or `async fn(Case)`); `driver_cases!(url body)` takes a
//! `fn(UrlCase)` for suites that spawn `openhuman-core` against the URL.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, MutexGuard};

use openhuman_core::storage::{self, StorageBackend};

/// The environment variable naming the MongoDB replica set the `mongodb`
/// cases run against.
pub const MONGO_URL_VAR: &str = "TSD_MONGO_URL";

/// One storage driver of the matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    Memory,
    Sqlite,
    File,
    Mongo,
}

impl Driver {
    /// Every driver, in the order the cases are declared.
    pub const ALL: [Driver; 4] = [Driver::Memory, Driver::Sqlite, Driver::File, Driver::Mongo];

    /// The driver's name, as in the test case path.
    pub fn name(self) -> &'static str {
        match self {
            Driver::Memory => "memory",
            Driver::Sqlite => "sqlite",
            Driver::File => "file",
            Driver::Mongo => "mongodb",
        }
    }

    /// Whether this build carries the driver.
    pub fn compiled_in(self) -> bool {
        match self {
            Driver::Memory => true,
            Driver::Sqlite => cfg!(feature = "storage-sqlite"),
            Driver::File => cfg!(feature = "storage-file"),
            Driver::Mongo => cfg!(feature = "storage-mongodb"),
        }
    }

    /// Whether records outlive the process that wrote them.
    pub fn is_durable(self) -> bool {
        self != Driver::Memory
    }

    /// The storage URL for this driver, using `dir` for the on-disk drivers.
    /// `None` when the driver cannot run here: not compiled in, or MongoDB
    /// without [`MONGO_URL_VAR`].
    pub fn url(self, dir: &std::path::Path) -> Option<String> {
        if !self.compiled_in() {
            return None;
        }
        match self {
            Driver::Memory => Some("memory".to_string()),
            Driver::Sqlite => Some(format!("sqlite:{}", dir.join("sqlite").display())),
            Driver::File => Some(format!("file:{}", dir.join("files").display())),
            Driver::Mongo => {
                let base = std::env::var(MONGO_URL_VAR)
                    .ok()
                    .filter(|url| !url.trim().is_empty())?;
                if !is_loopback_mongo_url(base.trim()) {
                    eprintln!(
                        "skipped mongodb: {MONGO_URL_VAR} must name a local throwaway server \
                         (localhost, 127.0.0.1 or ::1); the suite writes databases to it"
                    );
                    return None;
                }
                Some(mongo_url_with_database(
                    base.trim(),
                    &format!("oh_e2e_{}", uuid_like()),
                ))
            }
        }
    }
}

/// Whether every host in a MongoDB URL is the local machine. The matrix
/// creates and fills databases, so it only ever talks to a throwaway server
/// on this host or in this job's own container (as `tinystoragedrivers`' CI
/// does), never to a shared or remote deployment.
pub fn is_loopback_mongo_url(url: &str) -> bool {
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

/// `base` (a `mongodb://` or `mongodb+srv://` URL, with or without a database
/// and options) pointed at database `database`, keeping its host and query.
pub fn mongo_url_with_database(base: &str, database: &str) -> String {
    let (head, query) = match base.split_once('?') {
        Some((head, query)) => (head, Some(query)),
        None => (base, None),
    };
    let after_scheme = head.find("://").map_or(0, |at| at + 3);
    let authority_end = head[after_scheme..]
        .find('/')
        .map_or(head.len(), |at| after_scheme + at);
    let mut url = format!("{}/{database}", &head[..authority_end]);
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    url
}

/// A short unique token for database and scope names.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:x}_{}_{n}", std::process::id())
}

/// The slot is process-wide, so cases of one binary take turns.
static SLOT_LOCK: Mutex<()> = Mutex::new(());

/// One running case: a driver, its backend opened on a fresh directory, and
/// the lock on the process-wide storage slot. Dropping it empties the slot.
pub struct Case {
    pub driver: Driver,
    /// The URL the backend was opened from.
    pub url: String,
    /// The directory the on-disk drivers keep their data in.
    pub data_dir: tempfile::TempDir,
    backend: Arc<dyn StorageBackend>,
    _lock: MutexGuard<'static, ()>,
}

impl Case {
    /// Opens `driver`'s backend. `None` (after saying why on stderr) when the
    /// driver cannot run here.
    pub fn open(driver: Driver) -> Option<Case> {
        let lock = SLOT_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let data_dir = tempfile::tempdir().expect("temp dir");
        let Some(url) = driver.url(data_dir.path()) else {
            eprintln!(
                "skipped {}: {}",
                driver.name(),
                if driver.compiled_in() {
                    format!("{MONGO_URL_VAR} is not set")
                } else {
                    "driver not compiled in".to_string()
                }
            );
            return None;
        };
        let open_url = url.clone();
        // Opened on the storage bridge's runtime, the one the stores use, so
        // the backend's tasks outlive this call.
        let backend = storage::block_on(async move { storage::open(&open_url).await })
            .unwrap_or_else(|error| panic!("open the {} backend: {error}", driver.name()));
        Some(Case {
            driver,
            url,
            data_dir,
            backend,
            _lock: lock,
        })
    }

    /// The opened backend.
    pub fn backend(&self) -> Arc<dyn StorageBackend> {
        Arc::clone(&self.backend)
    }

    /// Makes the backend the process's storage backend.
    pub fn install(&self) {
        storage::install(self.backend());
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        storage::clear();
    }
}

/// A driver's URL for suites that hand it to a spawned `openhuman-core`
/// process instead of opening the backend in the test process.
pub struct UrlCase {
    pub driver: Driver,
    pub url: String,
    pub data_dir: tempfile::TempDir,
}

/// Runs a body that only needs the driver's URL (`driver_cases!(url body)`).
pub fn run_url(driver: Driver, body: fn(UrlCase)) {
    // Same lock as the in-process cases: the spawned cores and this process
    // share the environment and, for MongoDB, one server.
    let _lock = SLOT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let data_dir = tempfile::tempdir().expect("temp dir");
    let Some(url) = driver.url(data_dir.path()) else {
        eprintln!("skipped {}: {MONGO_URL_VAR} is not set", driver.name());
        return;
    };
    body(UrlCase {
        driver,
        url,
        data_dir,
    });
}

/// Runs a synchronous case body on `driver`.
pub fn run_sync(driver: Driver, body: fn(Case)) {
    if let Some(case) = Case::open(driver) {
        body(case);
    }
}

/// Runs an asynchronous case body on `driver` inside a multi-thread runtime.
pub fn run_async<F, Fut>(driver: Driver, body: F)
where
    F: FnOnce(Case) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    // Open before entering the runtime: `Case::open` blocks on the storage
    // bridge thread, which is fine here but not inside an async worker.
    if let Some(case) = Case::open(driver) {
        runtime.block_on(body(case));
    }
}

/// Declares one test case per driver for each `body`, in a module named after
/// the driver (`memory::body`, `sqlite::body`, ...). Drivers the build lacks
/// are not declared at all; MongoDB is declared when compiled in and skips
/// itself without `TSD_MONGO_URL`.
#[allow(unused_macros)]
macro_rules! driver_cases {
    (sync $($body:ident),+ $(,)?) => {
        driver_cases!(@each run_sync $($body),+);
    };
    (async $($body:ident),+ $(,)?) => {
        driver_cases!(@each run_async $($body),+);
    };
    (url $($body:ident),+ $(,)?) => {
        driver_cases!(@each run_url $($body),+);
    };
    (@each $runner:ident $($body:ident),+) => {
        mod memory {
            $(
                #[test]
                fn $body() {
                    $crate::storage_drivers::$runner(
                        $crate::storage_drivers::Driver::Memory,
                        super::$body,
                    );
                }
            )+
        }
        #[cfg(feature = "storage-sqlite")]
        mod sqlite {
            $(
                #[test]
                fn $body() {
                    $crate::storage_drivers::$runner(
                        $crate::storage_drivers::Driver::Sqlite,
                        super::$body,
                    );
                }
            )+
        }
        #[cfg(feature = "storage-file")]
        mod file {
            $(
                #[test]
                fn $body() {
                    $crate::storage_drivers::$runner(
                        $crate::storage_drivers::Driver::File,
                        super::$body,
                    );
                }
            )+
        }
        #[cfg(feature = "storage-mongodb")]
        mod mongodb {
            $(
                #[test]
                fn $body() {
                    $crate::storage_drivers::$runner(
                        $crate::storage_drivers::Driver::Mongo,
                        super::$body,
                    );
                }
            )+
        }
    };
}
