//! Seam integration tests — core behaviour whose *answer* is host routing.
//!
//! These moved out of `tinymemory-core` with the extraction. Each one calls
//! into the extracted crate but asserts something only the host decides: which
//! provider a role resolves to, what model id that yields, whether a
//! config-derived embedding signature matches the one the real provider
//! reports, whether Composio dispatches to the backend or direct tenant.
//!
//! A stub host could only assert itself, so they belong on this side, where the
//! real `ChatHost` / `EmbeddingHost` / `ComposioHost` implementations live.
//! See [`super::host_impls`].

#[cfg(test)]
#[path = "seam_integration_tests_tests.rs"]
mod tests;
