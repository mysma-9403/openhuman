use super::*;

#[test]
fn custom_delegate_is_treated_as_spawn_tool() {
    assert!(is_subagent_spawn_tool("spawn_subagent"));
    assert!(is_subagent_spawn_tool("delegate_researcher"));
    // Context scouting is top-level only — never visible to sub-agents
    // (incl. wildcard agents), which would otherwise scout the wrong
    // parent context. See #3949 review.
    assert!(is_subagent_spawn_tool("agent_prepare_context"));
    assert!(!is_subagent_spawn_tool("directory_resolve"));
}

#[test]
fn unprefixed_delegate_name_overrides_are_treated_as_spawn_tools() {
    // Most synthesised delegation tools use an unprefixed
    // `delegate_name` override (`plan`, `run_code`, `research`, …).
    // They must be stripped from every sub-agent surface, exactly like
    // the `delegate_*`-prefixed defaults.
    let tmp = tempfile::TempDir::new().unwrap();
    crate::openhuman::agent::harness::definition::AgentDefinitionRegistry::init_global(tmp.path())
        .unwrap();
    for delegate in [
        "plan",
        "run_code",
        "research",
        "review_code",
        "do_crypto",
        "schedule_task",
        // `make_presentation` is `presentation_agent`'s `delegate_name`; the agent —
        // and therefore this delegate tool — is compiled out with the
        // `documents` feature.
        #[cfg(feature = "documents")]
        "make_presentation",
        "archive_session",
        // `use_mcp_server` is `mcp_agent`'s `delegate_name`; the agent —
        // and therefore this delegate tool — is compiled out with the
        // `mcp` feature (#4799). `setup_mcp_server` belongs to
        // `mcp_setup`, which stays registered in both builds.
        #[cfg(feature = "mcp")]
        "use_mcp_server",
        "setup_mcp_server",
    ] {
        assert!(
            is_subagent_spawn_tool(delegate),
            "`{delegate}` is a synthesised delegation tool and must be \
             stripped from sub-agent tool surfaces"
        );
    }
    // Ordinary worker tools stay visible.
    for plain in ["shell", "file_read", "web_fetch", "todo"] {
        assert!(
            !is_subagent_spawn_tool(plain),
            "`{plain}` must not be classified as a spawn tool"
        );
    }
}
