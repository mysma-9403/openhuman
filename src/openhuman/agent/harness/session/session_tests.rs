//! `Agent` unit + integration tests.
//!
//! All tests exercise the agent through its public surface only (no
//! private-field access), which is why they live in a sibling file
//! rather than inline with one of the impl blocks. Shared fakes
//! (`MockProvider`, `RecordingProvider`, `MockTool`) are defined here.

use super::types::{Agent, AgentBuilder};
use crate::core::events::DomainEvent;
use crate::openhuman::agent::dispatcher::{NativeToolDispatcher, XmlToolDispatcher};
use crate::openhuman::agent::messages::ConversationMessage;
use crate::openhuman::inference::provider::ChatResponse;
use crate::openhuman::memory::Memory;
use crate::openhuman::tools::Tool;
use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use tinyinference::message::Message;
use tinyinference::model::{
    ChatModel, ModelProfile, ModelRequest, ModelResponse, ModelStream, ModelStreamItem,
};

struct MockProvider {
    responses: Mutex<Vec<ChatResponse>>,
}

#[async_trait]
impl ChatModel<()> for MockProvider {
    fn profile(&self) -> Option<&ModelProfile> {
        static PROFILE: std::sync::LazyLock<ModelProfile> =
            std::sync::LazyLock::new(ModelProfile::default);
        Some(&PROFILE)
    }

    async fn invoke(
        &self,
        _state: &(),
        request: ModelRequest,
    ) -> tinyinference::Result<ModelResponse> {
        let mut guard = self.responses.lock();
        let response = if guard.is_empty() {
            ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }
        } else {
            guard.remove(0)
        };
        Ok(
            crate::openhuman::agent::tinyagents::model::native_model_response_for_request(
                &response, &request,
            ),
        )
    }

    async fn stream(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference::Result<ModelStream> {
        let response = self.invoke(state, request).await?;
        Ok(Box::pin(futures::stream::iter(vec![
            ModelStreamItem::Started,
            ModelStreamItem::Completed(response),
        ])))
    }
}

/// Provider that records the system prompt bytes and model name of
/// every `chat()` call. Used by KV-cache stability tests — anything
/// that varies between turns (timestamps, re-rendered memory context,
/// flipped model hints) will show up as a diff between captures.
#[derive(Default)]
struct RecordingProvider {
    captures: Mutex<Vec<CapturedCall>>,
    responses: Mutex<Vec<ChatResponse>>,
}

#[derive(Clone)]
struct CapturedCall {
    system_prompt: Option<String>,
    model: String,
}

#[async_trait]
impl ChatModel<()> for RecordingProvider {
    fn profile(&self) -> Option<&ModelProfile> {
        static PROFILE: std::sync::LazyLock<ModelProfile> =
            std::sync::LazyLock::new(ModelProfile::default);
        Some(&PROFILE)
    }

    async fn invoke(
        &self,
        _state: &(),
        request: ModelRequest,
    ) -> tinyinference::Result<ModelResponse> {
        let system_prompt = request.messages.iter().find_map(|message| match message {
            Message::System(_) => Some(message.text()),
            _ => None,
        });
        self.captures.lock().push(CapturedCall {
            system_prompt,
            model: request.model.clone().unwrap_or_default(),
        });

        let mut guard = self.responses.lock();
        let response = if guard.is_empty() {
            ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            }
        } else {
            guard.remove(0)
        };
        Ok(
            crate::openhuman::agent::tinyagents::model::native_model_response_for_request(
                &response, &request,
            ),
        )
    }

    async fn stream(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference::Result<ModelStream> {
        let response = self.invoke(state, request).await?;
        Ok(Box::pin(futures::stream::iter(vec![
            ModelStreamItem::Started,
            ModelStreamItem::Completed(response),
        ])))
    }
}

struct MockTool;

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "echo"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
    ) -> Result<crate::openhuman::tools::ToolResult> {
        Ok(crate::openhuman::tools::ToolResult::success("tool-out"))
    }
}

// silence clippy — `AgentBuilder` is imported so tests can reference
// it in doc examples / type assertions if needed.
#[allow(dead_code)]
fn _assert_builder_is_exported() -> AgentBuilder {
    Agent::builder()
}

/// Minimal in-memory `Agent` build that every agent_definition_name
/// regression test reuses. Spins up a scratch workspace, a `none`
/// memory backend, a one-response `MockProvider`, and a single
/// `MockTool`, then feeds those into [`Agent::builder`]. Returns the
/// built `Agent` so individual tests can assert against the
/// [`Agent::agent_definition_name`] accessor.
fn build_minimal_agent_with_definition_name(definition_name: Option<&str>) -> Agent {
    // The embedding seam fails loudly when unwired; before the memory
    // extraction this was a direct call and needed no setup.
    crate::openhuman::memory::host_impls::install_for_tests();
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    let workspace_path = workspace.path().to_path_buf();

    let provider = Arc::new(MockProvider {
        responses: Mutex::new(vec![]),
    });

    let memory_cfg = crate::openhuman::config::MemoryConfig {
        backend: "none".into(),
        ..crate::openhuman::config::MemoryConfig::default()
    };
    let mem: Arc<dyn Memory> =
        Arc::from(tinymemory_core::store::create_memory(&memory_cfg, &workspace_path).unwrap());

    let mut builder = Agent::builder()
        .chat_model(provider)
        .tools(vec![Box::new(MockTool)])
        .memory(mem)
        .tool_dispatcher(Box::new(NativeToolDispatcher))
        .workspace_dir(workspace_path);

    if let Some(name) = definition_name {
        builder = builder.agent_definition_name(name);
    }

    builder.build().expect("minimal agent build should succeed")
}

fn integration_delegate_toolkit_enum(agent: &Agent) -> Vec<String> {
    let spec = agent
        .tool_specs()
        .iter()
        .find(|spec| spec.name == "delegate_to_integrations_agent")
        .expect("delegate_to_integrations_agent tool spec should be present");
    let mut out: Vec<String> = spec.parameters["properties"]["toolkit"]["enum"]
        .as_array()
        .expect("toolkit enum should be an array")
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect();
    out.sort();
    out
}

async fn turn_dispatches_spawn_subagent_through_full_path_inner() {
    // The embedding seam fails loudly when unwired; before the memory
    // extraction this was a direct call and needed no setup.
    crate::openhuman::memory::host_impls::install_for_tests();
    use crate::openhuman::agent::harness::AgentDefinitionRegistry;
    use crate::openhuman::tools::SpawnSubagentTool;

    // Idempotent — other tests may have already initialised it.
    AgentDefinitionRegistry::init_global_builtins().unwrap();

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    let workspace_path = workspace.path().to_path_buf();

    // Scripted responses, in the exact order MockProvider will see them:
    //   1. Parent turn iter 0 — emit a spawn_subagent tool call.
    //   2. Sub-agent (researcher) iter 0 — return final text "X is Y".
    //   3. Parent turn iter 1 — fold sub-agent result into "Based on the research, X is Y."
    let provider = Arc::new(MockProvider {
        responses: Mutex::new(vec![
            crate::openhuman::inference::provider::ChatResponse {
                text: Some(String::new()),
                tool_calls: vec![crate::openhuman::inference::provider::ToolCall {
                    id: "call-spawn".into(),
                    name: "spawn_subagent".into(),
                    arguments: serde_json::json!({
                        "agent_id": "__test_inherit_echo",
                        "prompt": "find out about X",
                        "blocking": true
                    })
                    .to_string(),
                    extra_content: None,
                }],
                usage: None,
                reasoning_content: None,
            },
            crate::openhuman::inference::provider::ChatResponse {
                text: Some("X is Y".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            },
            crate::openhuman::inference::provider::ChatResponse {
                text: Some("Based on the research, X is Y.".into()),
                tool_calls: vec![],
                usage: None,
                reasoning_content: None,
            },
        ]),
    });

    let memory_cfg = crate::openhuman::config::MemoryConfig {
        backend: "none".into(),
        ..crate::openhuman::config::MemoryConfig::default()
    };
    let mem: Arc<dyn Memory> =
        Arc::from(tinymemory_core::store::create_memory(&memory_cfg, &workspace_path).unwrap());

    // Tools include SpawnSubagentTool so the parent can call it.
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(SpawnSubagentTool::new())];

    let mut agent = Agent::builder()
        .chat_model(provider)
        .tools(tools)
        .memory(mem)
        .tool_dispatcher(Box::new(NativeToolDispatcher))
        .workspace_dir(workspace_path)
        .build()
        .unwrap();

    let response = agent.turn("tell me about X").await.unwrap();
    assert_eq!(response, "Based on the research, X is Y.");

    // The parent's history should contain the spawn_subagent
    // assistant tool call AND a tool-result message carrying the
    // sub-agent's compact output.
    let has_spawn_call = agent.history().iter().any(|msg| match msg {
        ConversationMessage::AssistantToolCalls { tool_calls, .. } => {
            tool_calls.iter().any(|c| c.name == "spawn_subagent")
        }
        _ => false,
    });
    assert!(
        has_spawn_call,
        "parent history should contain the spawn_subagent assistant tool call"
    );

    let tool_result_contains_subagent_output = agent.history().iter().any(|msg| match msg {
        ConversationMessage::ToolResults(results) => {
            results.iter().any(|r| r.content.contains("X is Y"))
        }
        ConversationMessage::Chat(chat) if chat.role == "tool" => chat.content.contains("X is Y"),
        _ => false,
    });
    assert!(
        tool_result_contains_subagent_output,
        "parent history should contain a tool-result entry with the sub-agent's output"
    );
}

// ─────────────────────────────────────────────────────────────────────
// S4: the transcript seam is genuinely substitutable
// ─────────────────────────────────────────────────────────────────────

/// A `SessionHistory` that keeps everything in memory and touches no file.
///
/// The point of the fake is not that it is convenient — it is that it is
/// *possible*. Before the locator existed, `session_history` was an
/// `Arc<dyn …>` the turn path constructed inline, so nothing could ever be put
/// behind it; this fake failing to compile or failing to receive the turn is
/// the regression signal for that.
struct FakeSessionHistory {
    path: std::path::PathBuf,
    canned: Option<crate::openhuman::agent::harness::session::transcript::SessionTranscript>,
    appended: Mutex<Vec<Vec<crate::openhuman::agent::messages::ChatMessage>>>,
}

impl crate::openhuman::agent::harness::session::transcript_history::SessionTranscriptRead
    for FakeSessionHistory
{
    fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn read_session(
        &self,
    ) -> Result<Option<crate::openhuman::agent::harness::session::transcript::SessionTranscript>>
    {
        Ok(self.canned.clone())
    }
}

impl crate::openhuman::agent::harness::session::transcript_history::SessionHistory
    for FakeSessionHistory
{
    fn append_turn(
        &self,
        turn: crate::openhuman::agent::harness::session::transcript_history::TranscriptTurn<'_>,
    ) -> Result<()> {
        self.appended.lock().push(turn.next.to_vec());
        Ok(())
    }
}

#[async_trait]
impl tinyagents_harness::memory::ChatHistory for FakeSessionHistory {
    async fn messages(&self, _thread_id: &str) -> tinyagents_harness::Result<Vec<Message>> {
        Ok(vec![])
    }
    async fn append(&self, _thread_id: &str, _message: Message) -> tinyagents_harness::Result<()> {
        Ok(())
    }
    async fn replace(
        &self,
        _thread_id: &str,
        _messages: Vec<Message>,
    ) -> tinyagents_harness::Result<()> {
        Ok(())
    }
    async fn clear(&self, _thread_id: &str) -> tinyagents_harness::Result<()> {
        Ok(())
    }
}

/// Serves one canned transcript for every lookup and one recording write
/// handle, so a whole session's transcript I/O can be observed off-disk.
struct FakeLocator {
    handle: Arc<FakeSessionHistory>,
}

impl crate::openhuman::agent::harness::session::transcript_history::SessionHistoryLocator
    for FakeLocator
{
    fn latest_for_agent(
        &self,
        _agent_name: &str,
    ) -> Option<
        Arc<dyn crate::openhuman::agent::harness::session::transcript_history::SessionTranscriptRead>,
    >{
        Some(self.handle.clone())
    }

    fn root_for_thread(
        &self,
        _thread_id: &str,
    ) -> Option<
        Arc<dyn crate::openhuman::agent::harness::session::transcript_history::SessionTranscriptRead>,
    >{
        Some(self.handle.clone())
    }

    fn open_stem(
        &self,
        _stem: &str,
        _seed: crate::openhuman::agent::harness::session::transcript::TranscriptMeta,
    ) -> Result<
        Arc<dyn crate::openhuman::agent::harness::session::transcript_history::SessionHistory>,
    > {
        Ok(self.handle.clone())
    }
}

fn fake_transcript_meta(
    thread_id: &str,
) -> crate::openhuman::agent::harness::session::transcript::TranscriptMeta {
    crate::openhuman::agent::harness::session::transcript::TranscriptMeta {
        agent_name: "faker".into(),
        agent_id: None,
        agent_type: Some("root".into()),
        dispatcher: "native".into(),
        provider: None,
        model: None,
        created: "2026-08-08T00:00:00Z".into(),
        updated: "2026-08-08T00:00:00Z".into(),
        turn_count: 1,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
        thread_id: Some(thread_id.into()),
        task_id: None,
    }
}

fn agent_with_fake_locator(
    workspace: &std::path::Path,
    canned: Option<crate::openhuman::agent::harness::session::transcript::SessionTranscript>,
) -> (Agent, Arc<FakeSessionHistory>) {
    let handle = Arc::new(FakeSessionHistory {
        path: workspace.join("session_raw").join("fake.jsonl"),
        canned,
        appended: Mutex::new(Vec::new()),
    });
    let memory_cfg = crate::openhuman::config::MemoryConfig {
        backend: "none".into(),
        ..crate::openhuman::config::MemoryConfig::default()
    };
    let mem: Arc<dyn Memory> =
        Arc::from(tinymemory_core::store::create_memory(&memory_cfg, workspace).unwrap());
    let agent = Agent::builder()
        .chat_model(Arc::new(MockProvider {
            responses: Mutex::new(vec![]),
        }))
        .tools(vec![Box::new(MockTool)])
        .memory(mem)
        .tool_dispatcher(Box::new(NativeToolDispatcher))
        .agent_definition_name("faker")
        .workspace_dir(workspace.to_path_buf())
        .with_session_history_locator(Arc::new(FakeLocator {
            handle: handle.clone(),
        }))
        .build()
        .expect("agent build should succeed");
    (agent, handle)
}

#[path = "session_tests_part_01_tests.rs"]
mod part_01_tests;
#[path = "session_tests_part_02_tests.rs"]
mod part_02_tests;
#[path = "session_tests_part_03_tests.rs"]
mod part_03_tests;
