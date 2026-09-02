use super::*;
use crate::openhuman::agent::context::prompt::{LearnedContextData, ToolCallFormat};
use std::collections::HashSet;

fn empty_ctx() -> PromptContext<'static> {
    use std::sync::OnceLock;
    static EMPTY_VISIBLE: OnceLock<HashSet<String>> = OnceLock::new();
    PromptContext {
        workspace_dir: std::path::Path::new("."),
        model_name: "test",
        agent_id: "crypto_agent",
        tools: &[],
        workflows: &[],
        dispatcher_instructions: "",
        learned: LearnedContextData::default(),
        visible_tool_names: EMPTY_VISIBLE.get_or_init(HashSet::new),
        tool_call_format: ToolCallFormat::PFormat,
        connected_integrations: &[],
        connected_identities_md: String::new(),
        include_profile: false,
        include_memory_md: false,
        curated_snapshot: None,
        user_identity: None,
        personality_soul_md: None,
        personality_memory_md: None,
        personality_roster: vec![],
        agents_md_global: None,
        agents_md_local: None,
    }
}

#[test]
fn build_returns_nonempty_body() {
    let body = build(&empty_ctx()).unwrap();
    assert!(!body.is_empty());
    assert!(body.contains("Crypto Agent"));
}

#[test]
fn build_enforces_read_simulate_confirm_execute() {
    let body = build(&empty_ctx()).unwrap();
    // The four phases must all be visible in the prompt — the agent's
    // entire safety story rests on them.
    assert!(
        body.contains("read, simulate, confirm, then execute")
            || body.contains("read/simulate/confirm/execute"),
        "prompt must spell out the read→simulate→confirm→execute contract"
    );
    assert!(
        body.contains("ask_user_clarification"),
        "prompt must require explicit user confirmation before execute"
    );
    assert!(
        body.contains("prepared_id"),
        "execute step must consume a prepared_id, not fabricated parameters"
    );
}

#[test]
fn build_forbids_fabrication_and_logging_secrets() {
    let body = build(&empty_ctx()).unwrap();
    assert!(
        body.contains("No fabrication"),
        "prompt must explicitly forbid fabricating chain/token/market params"
    );
    assert!(
        body.contains("Never log secrets") || body.contains("never log secrets"),
        "prompt must forbid echoing private keys / seed phrases"
    );
}
