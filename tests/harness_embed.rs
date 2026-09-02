//! End-to-end proof that `Harness` runs a real agent turn as a library call.
//!
//! This is the acceptance test `docs/plans/pluggable-core/phase-1-corebuilder.md`
//! specified and never got: build with no transport and no background services,
//! run one turn, and assert nothing was bound.
//!
//! # Why one test does all of it
//!
//! A `Harness` claims a process-wide slot, because the core's keyring, event bus
//! and `Once`-guarded subscribers are process-scoped. Splitting these assertions
//! into separate `#[test]` functions would either serialize them behind a mutex
//! (same thing, more code) or race. So the process builds exactly one harness
//! and checks everything against it.
//!
//! No live LLM call is made: `wiremock` stands in for the provider, which is
//! also what makes the routing assertion possible — if the turn had gone
//! anywhere else, the mock would have recorded no request.

use openhuman_core::core::runtime::{AGENT_WORKER_STACK_BYTES, MAX_BLOCKING_THREADS};
use openhuman_core::openhuman::config::Config;
use openhuman_core::{Access, Harness, Provider, Session, Workspace};
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPLY: &str = "harness-embed-ok";

/// An OpenAI-compatible chat completion carrying `content`.
fn chat_completion(content: &str) -> serde_json::Value {
    json!({
        "id": "chatcmpl-harness-embed",
        "object": "chat.completion",
        "created": 1_700_000_000_u64,
        "model": "harness-embed-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
    })
}

/// A config that keeps the turn offline: no local runtimes, no spaCy, no
/// embeddings endpoint. Mirrors `src/bin/library_profile/harness.rs::fixture()`,
/// which is the recipe already proven against real turns.
fn offline_config() -> Config {
    let mut config = Config::default();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    config.memory_tree.spacy_enabled = false;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
    config.default_temperature = 0.0;
    config
}

/// The tuned runtime the harness documents as the caller's responsibility.
///
/// A default 2 MiB worker stack overflows on a turn that delegates to a
/// sub-agent and aborts the whole process, so building it the documented way is
/// both what the test needs and a check that the documented way works.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(AGENT_WORKER_STACK_BYTES)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .build()
        .expect("tokio runtime")
}

#[test]
fn a_harness_runs_a_turn_against_the_provider_it_was_given() {
    let _ = env_logger::builder().is_test(true).try_init();

    runtime().block_on(async {
        // A stub backend. Not optional scenery: a harness that is not signed in
        // to the real backend still makes non-inference calls (the session
        // check, integrations), and a 401 from those publishes `SessionExpired`
        // — which fails the *next* turn's custom-provider gate for reasons
        // unrelated to the turn. Pointing the backend at a stub is what
        // `backend_url` exists for.
        let backend = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": { "id": "harness-embed-test", "email": "local@openhuman.local" }
            })))
            .mount(&backend)
            .await;

        let provider_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion(REPLY)))
            .mount(&provider_server)
            .await;

        let harness = Harness::builder()
            .config(offline_config())
            .workspace(Workspace::Ephemeral)
            .backend_url(backend.uri())
            .provider(
                Provider::openai_compatible(format!("{}/v1", provider_server.uri()), "sk-test")
                    .model("harness-embed-model"),
            )
            // Read-only: the turn has no business acting, and this keeps the
            // test from depending on the approval gate's timing.
            .access(Access::readonly())
            // Routing at a custom provider is gated on an active app session,
            // even though this harness was handed its own endpoint and key. A
            // local session satisfies that gate without asserting anything at
            // the backend — see `Session::local`.
            .session(Session::local("harness-embed-test"))
            .build()
            .await
            .expect("harness builds");

        // The workspace is the harness's own, not the operator's.
        let workspace_dir = harness.workspace_dir().to_path_buf();
        assert!(workspace_dir.is_dir(), "workspace was not created");
        assert!(
            !harness.action_dir().starts_with(&workspace_dir),
            "action_dir must not sit inside the workspace, or every agent write \
             is blocked by is_workspace_internal_path"
        );

        // No listener was bound: `ServiceSet` selects nothing that binds, and
        // `serve()` was never called.
        assert!(
            std::env::var("OPENHUMAN_CORE_RPC_URL").is_err(),
            "a library harness must not bind an RPC listener"
        );

        let first = harness.run("Say the magic word.").await.expect("turn runs");
        assert!(
            first.reply.contains(REPLY),
            "reply {:?} does not carry the provider's response",
            first.reply
        );
        assert!(
            !first.session_id.is_empty(),
            "the harness must mint a session id — the core returns none, so \
             without this a caller cannot continue a conversation at all"
        );

        // The turn went to the endpoint we named, not to the account's route.
        let requests = provider_server
            .received_requests()
            .await
            .expect("mock recorded requests");
        assert!(
            !requests.is_empty(),
            "the provider endpoint received nothing — the per-call route was ignored"
        );

        // Continuing a conversation reuses the caller's session id verbatim.
        let second = harness
            .turn("And again.")
            .session(&first.session_id)
            .send()
            .await
            .expect("second turn runs");
        assert_eq!(second.session_id, first.session_id);

        // The session database landed in the harness's workspace.
        assert!(
            workspace_dir.join("session_db/sessions.db").exists(),
            "sessions were not persisted under the harness workspace"
        );

        // A second harness in this process must be refused rather than silently
        // sharing process-global core state with the first.
        let err = Harness::builder()
            .workspace(Workspace::Ephemeral)
            .build()
            .await
            .expect_err("a second harness must be refused");
        assert!(
            matches!(err, openhuman_core::HarnessError::AlreadyRunning),
            "got {err:?}"
        );

        drop(harness);
        assert!(
            !workspace_dir.exists(),
            "an ephemeral workspace must be removed with its harness"
        );
    });
}
