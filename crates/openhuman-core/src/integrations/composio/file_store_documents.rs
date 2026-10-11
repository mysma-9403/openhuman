//! The Composio host state files on the `tinystoragedrivers` document port.
//!
//! Used instead of `<workspace>/integrations/<file>.json` when the host
//! configured a storage backend ([`crate::storage`]); `file_store.rs` picks it
//! per call, so the state lives under the acting agent's storage scope
//! (`local` on a single-user host). These are the connected-account identities
//! and the per-toolkit agent scope preferences: user data, so unlike a cache
//! they must follow the user to a shared backend.
//!
//! # Layout
//!
//! | Collection | Document id | Holds |
//! | --- | --- | --- |
//! | `composio_state` | the file name (`composio_identities.json`, ...) | `value`: the serde value the file held |

use std::path::Path;

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use tinystoragedrivers::{CollectionSpec, ErrorKind, Precondition, StorageError};

use crate::storage::documents::Repo;

const STATE: &str = "composio_state";
const DOMAIN: &str = "composio::file_store";

fn collections() -> Vec<CollectionSpec> {
    vec![CollectionSpec::new(STATE)]
}

/// The document store for this call, when the host configured one.
pub(super) fn current() -> Result<Option<Docs>> {
    #[cfg(test)]
    if let Some(docs) = TEST_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Ok(Some(docs));
    }
    Ok(Repo::current(DOMAIN, collections)?.map(Docs))
}

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<Option<Docs>> = const { std::cell::RefCell::new(None) };
}

fn id_of(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("composio state path {} has no file name", path.display()))
}

/// The composio state over one scoped document handle.
#[derive(Clone)]
pub(super) struct Docs(Repo);

impl Docs {
    #[cfg(test)]
    pub(super) fn over(scoped: &crate::storage::ScopedStorage) -> Self {
        Self(Repo::over(scoped, DOMAIN, collections))
    }

    /// The value stored for the file `path` names, or `T::default()`.
    pub(super) fn load<T: DeserializeOwned + Default>(&self, path: &Path) -> Result<T> {
        let id = id_of(path)?;
        let stored = self
            .0
            .run(|docs| async move { docs.get(STATE, &id).await })?;
        match stored.and_then(|stored| stored.doc.get("value").cloned()) {
            Some(value) => Ok(serde_json::from_value(value)?),
            None => Ok(T::default()),
        }
    }

    /// Read-modify-write of the value for the file `path` names, under
    /// compare-and-swap so two cores sharing the scope never lose each other's
    /// change. `change` returns its result and whether it changed the value
    /// (an unchanged value is not written); it is retried on a conflict, so it
    /// must be repeatable.
    pub(super) fn update<T, R>(
        &self,
        path: &Path,
        change: impl Fn(&mut T) -> (R, bool) + Send + 'static,
    ) -> Result<R>
    where
        T: DeserializeOwned + Serialize + Default + Send + 'static,
        R: Send + 'static,
    {
        let id = id_of(path)?;
        self.0.run(|docs| async move {
            for _ in 0..crate::storage::documents::CAS_ATTEMPTS {
                let stored = docs.get(STATE, &id).await?;
                let (mut value, precondition) = match &stored {
                    Some(stored) => (
                        stored
                            .doc
                            .get("value")
                            .cloned()
                            .map(serde_json::from_value)
                            .transpose()
                            .map_err(|e| StorageError::invalid_input(e.to_string()))?
                            .unwrap_or_default(),
                        stored.unchanged(),
                    ),
                    None => (T::default(), Precondition::Absent),
                };
                let (result, changed) = change(&mut value);
                if !changed {
                    return Ok(result);
                }
                let doc = json!({
                    "value": serde_json::to_value(&value)
                        .map_err(|e| StorageError::invalid_input(e.to_string()))?
                });
                match docs.put(STATE, &id, doc, precondition).await {
                    Ok(_) => return Ok(result),
                    Err(error) if error.kind() == ErrorKind::Conflict => {}
                    Err(error) => return Err(error),
                }
            }
            Err(StorageError::conflict(format!(
                "{STATE}/{id} kept changing under {} attempts",
                crate::storage::documents::CAS_ATTEMPTS
            )))
        })
    }

    pub(super) fn save<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let id = id_of(path)?;
        let doc = json!({ "value": serde_json::to_value(value)? });
        log::debug!("[composio:store] document save id={id}");
        self.0.run(|docs| async move {
            docs.put(STATE, &id, doc, Precondition::None)
                .await
                .map(|_| ())
        })
    }
}

#[cfg(test)]
#[path = "file_store_documents_tests.rs"]
mod tests;
