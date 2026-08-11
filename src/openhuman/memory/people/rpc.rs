//! Domain RPC handlers for people. Adapter handlers in `schemas.rs`
//! parse params and delegate here. Tests can call these functions
//! directly with a constructed `PeopleStore`.

use chrono::Utc;
use serde_json::{json, Value};

use crate::openhuman::memory::people::address_book::{AddressBookError, SystemContactsSource};
use crate::openhuman::memory::people::resolver::HandleResolver;
use crate::openhuman::memory::people::scorer::score;
use crate::openhuman::memory::people::store::PeopleStore;
use crate::openhuman::memory::people::types::{Handle, PersonId};
use crate::rpc::RpcOutcome;

/// List people ranked by composite score, highest first.
pub async fn handle_list(store: &PeopleStore, limit: usize) -> Result<RpcOutcome<Value>, String> {
    let limit = limit.clamp(1, 500);
    let people = store.list().await.map_err(|e| format!("list: {e}"))?;
    let now = Utc::now();
    let person_ids: Vec<PersonId> = people.iter().map(|p| p.id).collect();
    let interactions_by_person = store
        .batch_interactions_for(&person_ids)
        .await
        .map_err(|e| format!("batch_interactions_for: {e}"))?;

    let mut ranked: Vec<(Value, f32)> = Vec::with_capacity(people.len());
    for p in people {
        let interactions = interactions_by_person
            .get(&p.id)
            .cloned()
            .unwrap_or_default();
        let s = score(&interactions, now);
        let handles: Vec<Value> = p
            .handles
            .iter()
            .map(|h| {
                let (kind, value) = h.as_key();
                json!({ "kind": kind, "value": value })
            })
            .collect();
        ranked.push((
            json!({
                "person_id": p.id.to_string(),
                "display_name": p.display_name,
                "primary_email": p.primary_email,
                "primary_phone": p.primary_phone,
                "handles": handles,
                "score": s.score,
                "components": {
                    "recency": s.recency,
                    "frequency": s.frequency,
                    "reciprocity": s.reciprocity,
                    "depth": s.depth,
                },
                "interaction_count": interactions.len(),
            }),
            s.score,
        ));
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let people_json: Vec<Value> = ranked.into_iter().take(limit).map(|(v, _)| v).collect();
    Ok(RpcOutcome::new(json!({ "people": people_json }), vec![]))
}

/// Contacts whose most recent interaction is older than `days` days — the
/// "relationships going cold" surface the scorer's `ScoreComponents` doc
/// anticipates. Oldest last-touch first; contacts never interacted with are
/// excluded (there's nothing to drift from).
pub async fn handle_drifting(
    store: &PeopleStore,
    days: u64,
    limit: usize,
) -> Result<RpcOutcome<Value>, String> {
    let limit = limit.clamp(1, 500);
    let now = Utc::now();
    // Guard the u64 -> i64 conversion: `days as i64` wraps for values above
    // i64::MAX (u64::MAX -> -1), which would push the cutoff into the future and
    // return nearly every contact. Clamp with try_from, then subtract saturating
    // so an absurd threshold floors the cutoff (→ no matches) instead of
    // overflowing in debug builds.
    let days_i64 = i64::try_from(days).unwrap_or(i64::MAX);
    let cutoff_ts = now
        .timestamp()
        .saturating_sub(days_i64.saturating_mul(86_400));
    tracing::debug!(
        domain = "people",
        operation = "drifting",
        days,
        limit,
        cutoff_ts,
        "[people::rpc] drifting: querying contacts with no interaction in > {days}d"
    );
    let rows = store.list_drifting(cutoff_ts, limit).await.map_err(|e| {
        tracing::warn!(
            domain = "people",
            operation = "drifting",
            error = %e,
            "[people::rpc] drifting: list_drifting query failed"
        );
        format!("list_drifting: {e}")
    })?;
    tracing::debug!(
        domain = "people",
        operation = "drifting",
        count = rows.len(),
        "[people::rpc] drifting: {} contact(s) past the {days}d threshold",
        rows.len()
    );
    let contacts: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, display_name, primary_email, primary_phone, last_ts)| {
                let days_since_last = (now.timestamp() - last_ts).max(0) / 86_400;
                json!({
                    "person_id": id.to_string(),
                    "display_name": display_name,
                    "primary_email": primary_email,
                    "primary_phone": primary_phone,
                    "last_interaction_at": last_ts,
                    "days_since_last": days_since_last,
                })
            },
        )
        .collect();
    Ok(RpcOutcome::new(
        json!({ "contacts": contacts, "threshold_days": days }),
        vec![],
    ))
}

/// Resolve a handle to a `PersonId`. Mints on first sight when
/// `create_if_missing` is true.
pub async fn handle_resolve(
    store: &PeopleStore,
    handle: Handle,
    create_if_missing: bool,
) -> Result<RpcOutcome<Value>, String> {
    let resolver = HandleResolver::new(store);
    let existing = resolver.resolve(&handle).await?;
    let (result, created) = match (existing, create_if_missing) {
        (Some(id), _) => (Some(id), false),
        (None, true) => {
            let (id, created) = resolver.resolve_or_create_with_status(&handle).await?;
            (Some(id), created)
        }
        (None, false) => (None, false),
    };
    Ok(RpcOutcome::new(
        json!({
            "person_id": result.map(|p| p.to_string()),
            "created": created,
        }),
        vec![],
    ))
}

/// Seed the people store from the system address book (CNContactStore on
/// macOS). Triggers the TCC Contacts permission prompt if not yet granted.
///
/// Returns counts of seeded and skipped contacts, plus a `permission_denied`
/// flag so callers can surface an actionable message to the user.
pub async fn handle_refresh_address_book(store: &PeopleStore) -> Result<RpcOutcome<Value>, String> {
    let resolver = HandleResolver::new(store);
    let source = SystemContactsSource;
    match resolver.seed_from_address_book(&source).await {
        Ok((seeded, skipped)) => {
            tracing::debug!(
                "[people::rpc] refresh_address_book ok: seeded={seeded} skipped={skipped}"
            );
            Ok(RpcOutcome::new(
                json!({
                    "seeded": seeded,
                    "skipped": skipped,
                    "permission_denied": false,
                }),
                vec![],
            ))
        }
        Err(AddressBookError::PermissionDenied) => {
            tracing::warn!("[people::rpc] refresh_address_book: contacts permission denied");
            Ok(RpcOutcome::new(
                json!({
                    "seeded": 0,
                    "skipped": 0,
                    "permission_denied": true,
                }),
                vec![],
            ))
        }
        Err(AddressBookError::Other(e)) => Err(format!("address_book: {e}")),
    }
}

/// Return the component-broken-down score for one person.
pub async fn handle_score(
    store: &PeopleStore,
    person_id: PersonId,
) -> Result<RpcOutcome<Value>, String> {
    if store
        .get(person_id)
        .await
        .map_err(|e| format!("get_person: {e}"))?
        .is_none()
    {
        return Err(format!("person not found: {person_id}"));
    }
    let interactions = store
        .interactions_for(person_id)
        .await
        .map_err(|e| format!("interactions_for: {e}"))?;
    let s = score(&interactions, Utc::now());
    Ok(RpcOutcome::new(
        json!({
            "person_id": person_id.to_string(),
            "score": s.score,
            "components": {
                "recency": s.recency,
                "frequency": s.frequency,
                "reciprocity": s.reciprocity,
                "depth": s.depth,
            },
            "interaction_count": interactions.len(),
        }),
        vec![],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory::people::types::{Interaction, Person};
    use chrono::Duration;

    #[tokio::test]
    async fn list_orders_by_score_desc() {
        let store = PeopleStore::open_in_memory().unwrap();
        let now = Utc::now();

        // Person A: strong two-way conversation, recent.
        let a = PersonId::new();
        store
            .insert_person(
                &Person {
                    id: a,
                    display_name: Some("Alice".into()),
                    primary_email: Some("a@x.z".into()),
                    primary_phone: None,
                    handles: vec![],
                    created_at: now,
                    updated_at: now,
                },
                &[Handle::Email("a@x.z".into())],
            )
            .await
            .unwrap();
        for i in 0..10 {
            store
                .record_interaction(Interaction {
                    person_id: a,
                    ts: now - Duration::hours(i),
                    is_outbound: i % 2 == 0,
                    length: 300,
                })
                .await
                .unwrap();
        }

        // Person B: quiet, only one old outbound.
        let b = PersonId::new();
        store
            .insert_person(
                &Person {
                    id: b,
                    display_name: Some("Bob".into()),
                    primary_email: Some("b@x.z".into()),
                    primary_phone: None,
                    handles: vec![],
                    created_at: now,
                    updated_at: now,
                },
                &[Handle::Email("b@x.z".into())],
            )
            .await
            .unwrap();
        store
            .record_interaction(Interaction {
                person_id: b,
                ts: now - Duration::days(60),
                is_outbound: true,
                length: 20,
            })
            .await
            .unwrap();

        let outcome = handle_list(&store, 10).await.unwrap();
        let arr = outcome.value["people"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["display_name"], "Alice");
        assert_eq!(arr[1]["display_name"], "Bob");
        let alice_score = arr[0]["score"].as_f64().unwrap();
        let bob_score = arr[1]["score"].as_f64().unwrap();
        assert!(alice_score > bob_score);
    }

    #[tokio::test]
    async fn drifting_lists_stale_contacts_with_days_since() {
        let store = PeopleStore::open_in_memory().unwrap();
        let now = Utc::now();
        let mk = |name: &str| Person {
            id: PersonId::new(),
            display_name: Some(name.to_string()),
            primary_email: None,
            primary_phone: None,
            handles: vec![],
            created_at: now,
            updated_at: now,
        };
        let cold = mk("Cold");
        let warm = mk("Warm");
        store.insert_person(&cold, &[]).await.unwrap();
        store.insert_person(&warm, &[]).await.unwrap();
        store
            .record_interaction(Interaction {
                person_id: cold.id,
                ts: now - Duration::days(90),
                is_outbound: true,
                length: 5,
            })
            .await
            .unwrap();
        store
            .record_interaction(Interaction {
                person_id: warm.id,
                ts: now - Duration::days(2),
                is_outbound: true,
                length: 5,
            })
            .await
            .unwrap();

        // 30-day threshold: only `cold` (90d) drifts; `warm` (2d) is fresh.
        let outcome = handle_drifting(&store, 30, 100).await.unwrap();
        assert_eq!(outcome.value["threshold_days"], 30);
        let arr = outcome.value["contacts"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["display_name"], "Cold");
        let days_since = arr[0]["days_since_last"].as_i64().unwrap();
        assert!(
            (89..=91).contains(&days_since),
            "≈90 days since last touch, got {days_since}"
        );
    }

    #[tokio::test]
    async fn drifting_huge_days_threshold_does_not_overflow() {
        // A u64 `days` above i64::MAX must not wrap the cutoff into the future —
        // the old `days as i64` cast turned u64::MAX into -1, pushing the cutoff
        // ahead of `now` and returning everyone. The saturating cutoff floors to
        // the distant past instead, so nobody is "drifting past forever".
        let store = PeopleStore::open_in_memory().unwrap();
        let now = Utc::now();
        let p = Person {
            id: PersonId::new(),
            display_name: Some("Ancient".into()),
            primary_email: None,
            primary_phone: None,
            handles: vec![],
            created_at: now,
            updated_at: now,
        };
        store.insert_person(&p, &[]).await.unwrap();
        store
            .record_interaction(Interaction {
                person_id: p.id,
                ts: now - Duration::days(1000),
                is_outbound: true,
                length: 1,
            })
            .await
            .unwrap();

        let outcome = handle_drifting(&store, u64::MAX, 100).await.unwrap();
        let arr = outcome.value["contacts"].as_array().unwrap();
        assert!(
            arr.is_empty(),
            "u64::MAX days must floor the cutoff, not wrap it into the future"
        );
    }

    #[tokio::test]
    async fn resolve_without_create_returns_null_for_unknown() {
        let store = PeopleStore::open_in_memory().unwrap();
        let outcome = handle_resolve(&store, Handle::Email("x@y.z".into()), false)
            .await
            .unwrap();
        assert!(outcome.value["person_id"].is_null());
    }
}
