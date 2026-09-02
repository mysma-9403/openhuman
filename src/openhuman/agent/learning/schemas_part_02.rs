
// ── list_facets ───────────────────────────────────────────────────────────────

fn handle_list_facets(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use tinymemory_api::provider::FacetState;

        tracing::debug!("[learning.list_facets] called");

        let class_filter = params
            .get("class")
            .and_then(Value::as_str)
            .map(str::to_string);

        let cache = get_cache().await?;

        // list_all returns all states (active + provisional + candidate + dropped).
        let all = cache
            .list_all()
            .await
            .map_err(|e| format!("list_all failed: {e:#}"))?;

        let facets: Vec<serde_json::Value> = all
            .iter()
            .filter(|f| {
                // Expose Active and Provisional rows to the user.
                f.state == FacetState::Active || f.state == FacetState::Provisional
            })
            .filter(|f| {
                if let Some(cls) = &class_filter {
                    f.class.as_deref() == Some(cls.as_str())
                        || f.key.starts_with(&format!("{cls}/"))
                } else {
                    true
                }
            })
            .map(facet_to_json)
            .collect();

        let count = facets.len();
        let log = vec![format!(
            "learning.list_facets: returned {count} facets (class_filter={:?})",
            class_filter
        )];

        let payload = serde_json::json!({ "facets": facets, "count": count });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── get_facet ─────────────────────────────────────────────────────────────────

fn handle_get_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.get_facet] key={fk}");

        let cache = get_cache().await?;
        let facet = cache
            .get(&fk)
            .await
            .map_err(|e| format!("get failed: {e:#}"))?;

        let (found, facet_val) = match &facet {
            Some(f) => (true, facet_to_json(f)),
            None => (false, serde_json::Value::Null),
        };

        let log = vec![format!("learning.get_facet: key={fk} found={found}")];
        let payload = serde_json::json!({ "facet": facet_val, "found": found });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── update_facet ──────────────────────────────────────────────────────────────

fn handle_update_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use tinymemory_api::provider::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();
        let new_value = params
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `value`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.update_facet] key={fk} value={new_value}");

        let cache = get_cache().await?;

        let mut facet = cache
            .get(&fk)
            .await
            .map_err(|e| format!("get failed: {e:#}"))?
            .ok_or_else(|| format!("facet not found: {fk}"))?;

        // Update value and pin so this survives future rebuilds.
        facet.value = new_value.clone();
        facet.user_state = UserState::Pinned;

        cache
            .upsert(&facet)
            .await
            .map_err(|e| format!("upsert failed: {e:#}"))?;

        let updated = cache
            .get(&fk)
            .await
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after upsert".to_string())?;

        let log = vec![format!(
            "learning.update_facet: key={fk} new_value={new_value} user_state=pinned"
        )];
        let payload = serde_json::json!({ "facet": facet_to_json(&updated) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── pin_facet ─────────────────────────────────────────────────────────────────

fn handle_pin_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use tinymemory_api::provider::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.pin_facet] key={fk}");

        let cache = get_cache().await?;
        let updated = cache
            .set_user_state(&fk, UserState::Pinned)
            .await
            .map_err(|e| format!("set_user_state failed: {e:#}"))?;

        if !updated {
            return Err(format!("facet not found: {fk}"));
        }

        let facet = cache
            .get(&fk)
            .await
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after update".to_string())?;

        let log = vec![format!("learning.pin_facet: key={fk} user_state=pinned")];
        let payload = serde_json::json!({ "facet": facet_to_json(&facet) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── unpin_facet ───────────────────────────────────────────────────────────────

fn handle_unpin_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use tinymemory_api::provider::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.unpin_facet] key={fk}");

        let cache = get_cache().await?;
        let updated = cache
            .set_user_state(&fk, UserState::Auto)
            .await
            .map_err(|e| format!("set_user_state failed: {e:#}"))?;

        if !updated {
            return Err(format!("facet not found: {fk}"));
        }

        let facet = cache
            .get(&fk)
            .await
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after update".to_string())?;

        let log = vec![format!("learning.unpin_facet: key={fk} user_state=auto")];
        let payload = serde_json::json!({ "facet": facet_to_json(&facet) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── forget_facet ──────────────────────────────────────────────────────────────

fn handle_forget_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use tinymemory_api::provider::{FacetState, UserState};

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.forget_facet] key={fk}");

        let cache = get_cache().await?;

        let facet_before = cache
            .get(&fk)
            .await
            .map_err(|e| format!("get failed: {e:#}"))?;

        let facet_json = if let Some(mut f) = facet_before {
            // Mark Forgotten + Dropped so it doesn't resurface.
            f.user_state = UserState::Forgotten;
            f.state = FacetState::Dropped;
            cache
                .upsert(&f)
                .await
                .map_err(|e| format!("upsert failed: {e:#}"))?;
            let updated = cache
                .get(&fk)
                .await
                .map_err(|e| format!("re-read failed: {e:#}"))?
                .unwrap_or(f);
            facet_to_json(&updated)
        } else {
            serde_json::Value::Null
        };

        let log = vec![format!(
            "learning.forget_facet: key={fk} state=dropped user_state=forgotten"
        )];
        let payload = serde_json::json!({ "facet": facet_json });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── reset_cache ───────────────────────────────────────────────────────────────

fn handle_reset_cache(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        tracing::debug!("[learning.reset_cache] called");

        let cache = get_cache().await?;

        let (deleted, pinned_preserved) =
            crate::openhuman::agent::learning::cache::reset_non_pinned(&cache)
                .await
                .map_err(|e| format!("reset_cache failed: {e:#}"))?;

        tracing::info!(
            "[learning.reset_cache] deleted={deleted} pinned_preserved={pinned_preserved}"
        );

        let log = vec![format!(
            "learning.reset_cache: deleted={deleted} pinned_preserved={pinned_preserved}"
        )];
        let payload = serde_json::json!({
            "deleted": deleted,
            "pinned_preserved": pinned_preserved,
        });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}
