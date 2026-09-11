//! Real HTTP adapter + durable recovery; provider traffic stays on loopback.
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{json, Value};
use zork_agent::session::events::{Selection, TurnOutcome};
use zork_agent::session::state::GenerationEntry;
use zork_agent_testkit::RealAgent;

fn profile(base: &str, id: &str) -> Value {
    json!({"provider":"openai-compatible","billing":"usage","base_url":format!("{base}/v1"),
        "auth":{"type":"api_key","key":format!("secret-{id}")},
        "headers":{"x-account":id},
        "models":[{"id":"pool-model","api":"openai-completions","thinking":["high"],
            "default_thinking":"high","capabilities":{"input":["text"]},
            "limits":{"context_window_tokens":100000,"max_output_tokens":10000},"default":true}]})
}

#[tokio::test]
async fn omitted_profile_and_effort_alias_preserve_auto_intent_across_selection_updates() {
    let mut agent = RealAgent::new().unwrap();
    agent
        .install_profile("a", profile(agent.provider_base_url(), "a"))
        .unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let response = agent
        .client()
        .post(format!("{}/sessions", agent.base_url()))
        .json(&json!({"model":"pool-model","effort":"high","workspace":workspace.path()}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: Value = response.json().await.unwrap();
    assert_eq!(created["profile_id"], "auto");
    let id = created["session_id"].as_str().unwrap();
    for (selection, expected) in [
        (
            json!({"profile_id":"a","model":"pool-model","thinking":"high"}),
            "a",
        ),
        (json!({"model":"pool-model","effort":"high"}), "auto"),
    ] {
        let response = agent
            .client()
            .put(format!("{}/sessions/{id}/selection", agent.base_url()))
            .json(&selection)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["profile_id"],
            expected
        );
    }
    let bad = agent
        .client()
        .post(format!("{}/sessions", agent.base_url()))
        .json(&json!({"model":"missing","effort":"high"}))
        .send()
        .await
        .unwrap();
    assert!(!bad.status().is_success());
    agent.shutdown().await;
}

#[tokio::test]
async fn quota_rejection_switches_real_provider_and_restart_retains_actual_account() {
    let mut agent = RealAgent::new().unwrap();
    for id in ["a", "b"] {
        agent
            .install_profile(id, profile(agent.provider_base_url(), id))
            .unwrap();
    }
    let id = agent
        .create_configured_session(
            Selection {
                profile_id: "auto".into(),
                model: "pool-model".into(),
                thinking: "high".into(),
            },
            None,
        )
        .await
        .unwrap();
    agent.send_mail(&id, "first turn").await.unwrap();
    let first = agent.request().await;
    assert_eq!(first.headers["x-account"], "a");
    first.respond_json(StatusCode::TOO_MANY_REQUESTS,json!({"error":{"message":"quota exhausted","type":"insufficient_quota","code":"insufficient_quota"}})).unwrap();
    let second = agent.request().await;
    assert_eq!(second.headers["x-account"], "b");
    second
        .respond_openai_calls("done-1", [("end-1", "end", json!({}))])
        .unwrap();
    let settled = agent
        .wait_for_state(&id, |s| s.last_turn_outcome == Some(TurnOutcome::Finished))
        .await;
    assert_eq!(settled.selection.as_ref().unwrap().profile_id, "auto");
    assert!(settled.generation.entries.iter().any(|entry| matches!(entry,
        GenerationEntry::Assistant { provider_context:Some(context), .. } if context.profile_id == "b")));
    // Runtime projection moved from history pages to the initial SSE snapshot.
    let mut events = agent
        .client()
        .get(format!("{}/sessions/{id}/events", agent.base_url()))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes_stream();
    let snapshot: Value = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut buffer = Vec::new();
        loop {
            buffer.extend(events.next().await.unwrap().unwrap());
            if let Some(end) = buffer.windows(2).position(|bytes| bytes == b"\n\n") {
                let frame = std::str::from_utf8(&buffer[..end]).unwrap();
                assert!(frame.lines().any(|line| line == "event: snapshot"));
                break serde_json::from_str(
                    frame
                        .lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .unwrap(),
                )
                .unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(snapshot["runtime"]["profile"]["profile_id"], "b");
    assert!(!snapshot.to_string().contains("secret-"));
    drop(events);
    agent.restart().await.unwrap();
    agent.send_mail(&id, "second turn").await.unwrap();
    let resumed = agent.request().await;
    assert_eq!(resumed.headers["x-account"], "b");
    assert!(resumed.json().unwrap().to_string().contains("first turn"));
    resumed
        .respond_openai_calls("done-2", [("end-2", "end", json!({}))])
        .unwrap();
    agent
        .wait_for_state(&id, |s| {
            s.last_turn_outcome == Some(TurnOutcome::Finished) && s.active_turn.is_none()
        })
        .await;
    agent.shutdown().await;
}
