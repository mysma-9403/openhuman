//! Small JSON files under the workspace for the Composio host state that is
//! not memory: connected-account identities and per-toolkit scope prefs.
//!
//! Each file holds one serde value. A read of a missing file is the default
//! value; a write goes to a sibling temp file first and is renamed over the
//! target, so a crash mid-write leaves the previous file intact. Every
//! read-modify-write runs under [`lock`], one process-wide async mutex, so two
//! concurrent updates cannot lose each other's change.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{Mutex, MutexGuard};

use crate::config::Config;

/// Directory under the workspace holding the integration state files.
const DIR: &str = "integrations";

/// Connected-account identities file name.
pub(crate) const IDENTITIES_FILE: &str = "composio_identities.json";

/// Per-toolkit agent scope prefs file name.
pub(crate) const USER_SCOPES_FILE: &str = "composio_user_scopes.json";

/// `<workspace>/integrations/<file>`.
pub(crate) fn path(config: &Config, file: &str) -> PathBuf {
    config.workspace_dir.join(DIR).join(file)
}

/// Serialises read-modify-write cycles over the integration state files.
pub(crate) async fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::const_new(());
    LOCK.lock().await
}

/// The value stored at `path`, or `T::default()` when the file does not exist.
///
/// # Errors
///
/// The file exists but cannot be read or does not parse as `T`.
pub(crate) async fn load<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned + Default + Send + 'static,
{
    if let Some(docs) =
        super::file_store_documents::current().map_err(|e| format!("[composio:store] {e:#}"))?
    {
        // The document port is reached through a blocking bridge; keep it off
        // the async worker, under the caller's context.
        let owned = path.to_path_buf();
        return crate::core::runtime::spawn_blocking_scoped(move || docs.load(&owned))
            .await
            .map_err(|error| {
                format!(
                    "[composio:store] reading {} failed: {error}",
                    path.display()
                )
            })?
            .map_err(|error| {
                format!(
                    "[composio:store] reading {} failed: {error:#}",
                    path.display()
                )
            });
    }
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => {
            return Err(format!(
                "[composio:store] reading {} failed: {error}",
                path.display()
            ))
        }
    };
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "[composio:store] parsing {} failed: {error}",
            path.display()
        )
    })
}

/// Applies `change` to the value at `path` and writes it back when `change`
/// reports a change, returning its result.
///
/// With a storage backend this is a compare-and-swap loop, so two cores that
/// share the scope never lose each other's update (`change` is retried on a
/// conflict and must be repeatable). Without one it is the process-wide
/// [`lock`] around a load and a save.
///
/// # Errors
///
/// The value cannot be read or written.
pub(crate) async fn update<T, R>(
    path: &Path,
    change: impl Fn(&mut T) -> (R, bool) + Send + 'static,
) -> Result<R, String>
where
    T: DeserializeOwned + Serialize + Default + Send + 'static,
    R: Send + 'static,
{
    if let Some(docs) =
        super::file_store_documents::current().map_err(|e| format!("[composio:store] {e:#}"))?
    {
        let owned = path.to_path_buf();
        return crate::core::runtime::spawn_blocking_scoped(move || docs.update(&owned, change))
            .await
            .map_err(|error| {
                format!(
                    "[composio:store] updating {} failed: {error}",
                    path.display()
                )
            })?
            .map_err(|error| {
                format!(
                    "[composio:store] updating {} failed: {error:#}",
                    path.display()
                )
            });
    }
    let _guard = lock().await;
    let mut value: T = load(path).await?;
    let (result, changed) = change(&mut value);
    if changed {
        save(path, &value).await?;
    }
    Ok(result)
}

/// Writes `value` to `path` atomically (temp file, then rename).
///
/// # Errors
///
/// The directory cannot be created, or the write or rename fails.
pub(crate) async fn save<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    if let Some(docs) =
        super::file_store_documents::current().map_err(|e| format!("[composio:store] {e:#}"))?
    {
        let owned = path.to_path_buf();
        let value = serde_json::to_value(value)
            .map_err(|error| format!("[composio:store] serializing failed: {error}"))?;
        return crate::core::runtime::spawn_blocking_scoped(move || docs.save(&owned, &value))
            .await
            .map_err(|error| {
                format!(
                    "[composio:store] writing {} failed: {error}",
                    path.display()
                )
            })?
            .map_err(|error| {
                format!(
                    "[composio:store] writing {} failed: {error:#}",
                    path.display()
                )
            });
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("[composio:store] serializing failed: {error}"))?;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|error| {
            format!(
                "[composio:store] creating {} failed: {error}",
                dir.display()
            )
        })?;
    }
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|error| format!("[composio:store] writing {} failed: {error}", tmp.display()))?;
    tokio::fs::rename(&tmp, path).await.map_err(|error| {
        format!(
            "[composio:store] replacing {} failed: {error}",
            path.display()
        )
    })?;
    tracing::debug!(path = %path.display(), bytes = bytes.len(), "[composio:store] saved");
    Ok(())
}

#[cfg(test)]
#[path = "file_store_tests.rs"]
mod tests;
