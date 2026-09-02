use super::*;
use crate::openhuman::skills::{Workflow, WorkflowFrontmatter, WorkflowScope};
use tempfile::tempdir;

fn skill(name: &str, location: Option<std::path::PathBuf>) -> Workflow {
    Workflow {
        name: name.to_string(),
        dir_name: name.to_string(),
        description: format!("{name} description"),
        version: "1.0.0".into(),
        author: None,
        tags: vec![],
        platforms: vec![],
        related_skills: vec![],
        source_format: "openhuman".to_string(),
        tools: vec![],
        prompts: vec![],
        location,
        frontmatter: WorkflowFrontmatter::default(),
        resources: vec![],
        scope: WorkflowScope::Project,
        legacy: false,
        warnings: vec![],
    }
}

#[test]
fn inject_workspace_file_truncates_at_utf8_boundary_and_emits_read_hint() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("SOUL.md"), "é".repeat(10)).unwrap();

    let mut prompt = String::new();
    inject_workspace_file(&mut prompt, tmp.path(), "SOUL.md", 5);

    assert!(prompt.contains("### SOUL.md"));
    assert!(prompt.contains("truncated at 5 chars"));
    assert!(prompt.is_char_boundary(prompt.len()));
    assert!(prompt.contains("read` for full file") || prompt.contains("read` for full file]"));
}

#[test]
fn inject_workspace_file_emits_missing_marker_for_bootstrap_files() {
    let tmp = tempdir().unwrap();
    let mut prompt = String::new();

    inject_workspace_file(&mut prompt, tmp.path(), "IDENTITY.md", 100);

    assert!(prompt.contains("[File not found: IDENTITY.md]"));
}

#[test]
fn load_openclaw_bootstrap_files_omits_missing_optional_files() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("SOUL.md"), "Soul").unwrap();
    std::fs::write(tmp.path().join("IDENTITY.md"), "Identity").unwrap();

    let mut prompt = String::new();
    load_openclaw_bootstrap_files(&mut prompt, tmp.path(), 100);

    assert!(prompt.contains("### SOUL.md"));
    assert!(prompt.contains("### IDENTITY.md"));
    assert!(!prompt.contains("PROFILE.md"));
    assert!(!prompt.contains("MEMORY.md"));
}

#[test]
fn build_system_prompt_uses_generic_channel_copy_when_channel_name_absent() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("SOUL.md"), "Soul").unwrap();
    std::fs::write(tmp.path().join("IDENTITY.md"), "Identity").unwrap();

    let prompt = build_system_prompt(tmp.path(), "test-model", &[], &[], None, None);

    assert!(prompt.contains("You are running as a messaging bot."));
    assert!(prompt.contains(
        "When someone messages you, your response is automatically sent back to the same platform."
    ));
    assert!(!prompt.contains("running as a Discord bot"));
}

#[test]
fn build_system_prompt_uses_workspace_skill_location_fallback() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("SOUL.md"), "Soul").unwrap();
    std::fs::write(tmp.path().join("IDENTITY.md"), "Identity").unwrap();

    let prompt = build_system_prompt(
        tmp.path(),
        "test-model",
        &[("search", "Find facts")],
        &[skill("research-helper", None)],
        None,
        Some("Telegram"),
    );

    assert!(prompt.contains("<name>research-helper</name>"));
    assert!(prompt.contains(
        &tmp.path()
            .join("skills")
            .join("research-helper")
            .join("SKILL.md")
            .display()
            .to_string()
    ));
    assert!(prompt.contains("running as a Telegram bot"));
}
