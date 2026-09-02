use super::*;
use crate::openhuman::agent::task_board::{TaskBoardCard, TaskCardStatus};

fn card(title: &str) -> TaskBoardCard {
    TaskBoardCard {
        id: "card-1".to_string(),
        title: title.to_string(),
        status: TaskCardStatus::InProgress,
        objective: None,
        plan: Vec::new(),
        assigned_agent: None,
        allowed_tools: Vec::new(),
        approval_mode: None,
        acceptance_criteria: Vec::new(),
        evidence: Vec::new(),
        notes: None,
        blocker: None,
        session_thread_id: None,
        source_metadata: None,
        order: 0,
        updated_at: String::new(),
    }
}

fn temp_ws() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("task-session-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn creates_top_level_tasks_thread_and_seeds_prompt() {
    let ws = temp_ws();
    let id = create_session_thread(
        ws.clone(),
        &card("Design the onboarding"),
        "run-1",
        "Do the thing",
    )
    .expect("thread created");

    // Top-level (no parent) + labelled `tasks` so it lands in the Tasks tab.
    let threads = conversations::list_threads(ws.clone()).expect("list threads");
    let t = threads.iter().find(|t| t.id == id).expect("thread listed");
    assert!(
        t.parent_thread_id.is_none(),
        "session thread must be top-level"
    );
    assert!(
        t.labels.iter().any(|l| l == "tasks"),
        "must carry the tasks label"
    );

    // Seed user message carries the prompt + correlation metadata.
    let msgs = conversations::get_messages(ws, &id).expect("messages");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].sender, "user");
    assert_eq!(msgs[0].content, "Do the thing");
}

#[test]
fn append_final_writes_assistant_outcome() {
    let ws = temp_ws();
    let id = create_session_thread(ws.clone(), &card("X"), "run-2", "prompt").expect("thread");
    append_final(ws.clone(), &id, &Ok("All done.".to_string()));

    let msgs = conversations::get_messages(ws, &id).expect("messages");
    let last = msgs.last().expect("has messages");
    assert_eq!(last.sender, "assistant");
    assert_eq!(last.content, "All done.");
}

#[test]
fn append_final_skips_empty_response() {
    let ws = temp_ws();
    let id = create_session_thread(ws.clone(), &card("X"), "run-3", "prompt").expect("thread");
    append_final(ws.clone(), &id, &Ok("   ".to_string()));

    let msgs = conversations::get_messages(ws, &id).expect("messages");
    assert_eq!(
        msgs.len(),
        1,
        "empty final response must not append a message"
    );
}

#[test]
fn empty_title_falls_back_to_generic_label() {
    assert_eq!(session_title(&card("   ")), "Autonomous task");
    assert_eq!(session_title(&card("Real title")), "Real title");
}
