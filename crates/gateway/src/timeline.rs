use anyhow::Result;
use serde_json::{json, Value};

use crate::db::{GatewayDb, SessionRow};

pub fn load_page(
    db: &GatewayDb,
    session: &SessionRow,
    limit: usize,
    before_sequence: Option<u64>,
) -> Result<Value> {
    let events = gateway_events(db, session)?;
    let mut newest_first = events.clone();
    newest_first.reverse();
    let bounded: Vec<Value> = newest_first
        .into_iter()
        .filter(|event| {
            before_sequence
                .map(|before| event["sequence"].as_u64().unwrap_or(0) < before)
                .unwrap_or(true)
        })
        .collect();
    let mut selected: Vec<Value> = bounded.iter().take(limit).cloned().collect();
    let next_before = selected
        .iter()
        .filter_map(|event| event["sequence"].as_u64())
        .min();
    selected.reverse();
    Ok(json!({
        "summary": summarize(&events),
        "hasMore": bounded.len() > selected.len(),
        "nextBeforeSequence": next_before,
        "events": selected,
    }))
}

pub fn load_event(db: &GatewayDb, session: &SessionRow, event_id: &str) -> Result<Option<Value>> {
    Ok(gateway_events(db, session)?
        .into_iter()
        .find(|event| event["id"].as_str() == Some(event_id)))
}

fn gateway_events(db: &GatewayDb, session: &SessionRow) -> Result<Vec<Value>> {
    let mut events = vec![json!({
        "id": "session-created",
        "type": "session_created",
        "sessionKey": session.key,
        "title": "Session created",
        "at": session.created_at,
    })];

    for inbound in db.list_inbound(&session.key)? {
        events.push(json!({
            "id": format!("inbound:{}", inbound.message_ts),
            "type": "inbound_message",
            "sessionKey": session.key,
            "source": inbound.source,
            "userId": inbound.user_id,
            "title": inbound.text,
            "detail": inbound.text,
            "status": inbound.status,
            "at": inbound.created_at,
            "updatedAt": inbound.updated_at,
        }));
    }

    for job in db.list_jobs_for_session(&session.key)? {
        events.push(json!({
            "id": format!("job:{}", job.id),
            "type": "background_job",
            "sessionKey": session.key,
            "jobId": job.id,
            "kind": job.kind,
            "title": job.kind,
            "status": job.status,
            "at": job.created_at,
            "updatedAt": job.updated_at,
        }));
    }

    events.sort_by(|left, right| {
        left["at"]
            .as_str()
            .cmp(&right["at"].as_str())
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence"] = json!(index + 1);
    }
    Ok(events)
}

fn summarize(events: &[Value]) -> Value {
    let mut categories = serde_json::Map::new();
    for event in events {
        let kind = event["type"].as_str().unwrap_or("unknown");
        let current = categories.get(kind).and_then(Value::as_u64).unwrap_or(0);
        categories.insert(kind.to_owned(), json!(current + 1));
    }
    json!({
        "source": "gateway_db",
        "eventCount": events.len(),
        "categories": categories,
    })
}
