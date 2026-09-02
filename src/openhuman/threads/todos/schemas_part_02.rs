
fn handle_reclaim_stale(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ReclaimStaleParams>(params)?;
        let loc = thread_location(&p.thread_id).await?;
        let limits = runs::RunLimits {
            heartbeat_stale_secs: p
                .heartbeat_stale_secs
                .unwrap_or(runs::DEFAULT_HEARTBEAT_STALE_SECS),
            claim_ttl_secs: p.claim_ttl_secs.unwrap_or(runs::DEFAULT_CLAIM_TTL_SECS),
            max_reclaim_count: p
                .max_reclaim_count
                .unwrap_or(runs::DEFAULT_MAX_RECLAIM_COUNT),
        };
        tracing::debug!(
            thread_id = %p.thread_id,
            ?limits,
            "[rpc][todos] reclaim_stale entry"
        );
        let result = runs::reclaim_stale(&loc, &limits).await?;
        serde_json::to_value(&result).map_err(|e| format!("serialize reclaim result: {e}"))
    })
}

// ── helpers ──────────────────────────────────────────────────────────

async fn thread_location(thread_id: &str) -> Result<BoardLocation, String> {
    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err("thread_id must not be empty".to_string());
    }
    let config = crate::openhuman::config::Config::load_or_init()
        .await
        .map_err(|e| format!("load config: {e}"))?;
    Ok(BoardLocation::Thread {
        workspace_dir: config.workspace_dir,
        thread_id: trimmed.to_string(),
    })
}

fn parse<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn snapshot_to_json(snap: TodosSnapshot) -> Result<Value, String> {
    serde_json::to_value(&snap).map_err(|e| format!("serialize snapshot: {e}"))
}

fn thread_id_input() -> FieldSchema {
    FieldSchema {
        name: "thread_id",
        ty: TypeSchema::String,
        comment: "Conversation thread identifier (same id used by `threads.task_board_*`).",
        required: true,
    }
}

fn required_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::String,
        comment,
        required: true,
    }
}

fn optional_string(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::String)),
        comment,
        required: false,
    }
}

fn string_array_input(name: &'static str, comment: &'static str) -> FieldSchema {
    FieldSchema {
        name,
        ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(TypeSchema::String)))),
        comment,
        required: false,
    }
}

fn snapshot_output() -> FieldSchema {
    FieldSchema {
        name: "snapshot",
        ty: TypeSchema::Json,
        comment: "Object with `threadId`, `cards`, and a `markdown` rendering of the list.",
        required: true,
    }
}
