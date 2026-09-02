//! JSON-RPC surface for the durable agent session database.
//!
//! The store itself — sessions, messages, tool calls, cost metadata,
//! parent/child lineage, and the run ledger — lives in
//! [`tinyagents::session`]. Only the controller schemas and
//! their handlers stay here, because the RPC envelope, config resolution, and
//! `RpcOutcome` shape are host concerns the runtime crate has no business
//! knowing about.
//!
//! Call the store directly (`tinyagents_session::…`) rather
//! than through this module; it deliberately re-exports no storage API.
//!
//! Every store entry point takes the workspace root, so handlers pass
//! `config.workspace_dir`. The database path is
//! `{workspace}/session_db/sessions.db` — unchanged by the move, so existing
//! installs keep their history.
//!
//! The `session_db` and `run_ledger` RPC namespaces are unchanged.

mod schemas;

pub use schemas::{
    all_controller_schemas as all_session_db_controller_schemas,
    all_registered_controllers as all_session_db_registered_controllers,
};
