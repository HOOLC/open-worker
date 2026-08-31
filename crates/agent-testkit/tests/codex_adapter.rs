use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zork_agent::session::events::{SessionEvent, TurnOutcome};
use zork_agent::session::wire::SessionSelection;
use zork_agent_testkit::{ControlledCodexProvider, PendingCodexRequest, RealAgent};

const MODEL: &str = "controlled-codex-model";

fn selection(profile_id: &str) -> SessionSelection {
    SessionSelection {
        profile_id: profile_id.into(),
        model: MODEL.into(),
        thinking: "max".into(),
    }
}

fn profile(provider_base_url: &str, context_window_tokens: u64, max_output_tokens: u64) -> Value {
    json!({
        "provider": "openai",
        "billing": "subscription",
        "base_url": provider_base_url,
        "headers": {"x-codex-fixture": "present"},
        "auth": {"type": "api_key", "key": "codex-secret"},
        "models": [{
            "id": MODEL,
            "api": "openai-codex-responses",
            "streaming": true,
            "service_tier": "priority",
            "parallel_tool_calls": false,
            "thinking": ["max"],
            "default_thinking": "max",
            "capabilities": {"input": ["text"]},
            "limits": {
                "context_window_tokens": context_window_tokens,
                "max_output_tokens": max_output_tokens
            },
            "default": true
        }]
    })
}

fn call_item(item_id: &str, call_id: &str, tool: &str, arguments: Value) -> Value {
    json!({
        "id": item_id,
        "type": "function_call",
        "status": "completed",
        "arguments": json!({"tool": tool, "arguments": arguments}).to_string(),
        "call_id": call_id,
        "name": "call",
        "namespace": "functions"
    })
}

fn reasoning_item() -> Value {
    json!({
        "id": "rs_parallel",
        "type": "reasoning",
        "status": "completed",
        "encrypted_content": "encrypted-parallel",
        "summary": [{"type": "summary_text", "text": "Run all requested tools."}],
        "provider_extension": {"preserve": true}
    })
}

fn text_item(item_id: &str, text: &str) -> Value {
    json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": []}]
    })
}

async fn finish(request: PendingCodexRequest, suffix: &str) {
    request
        .respond(
            &format!("resp_end_{suffix}"),
            &[call_item(
                &format!("fc_end_{suffix}"),
                &format!("call_end_{suffix}"),
                "end",
                json!({}),
            )],
            10,
            2,
        )
        .await
        .unwrap();
}

async fn wait_for_file(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("tool created its start marker");
}

fn request_input(request: &PendingCodexRequest) -> &[Value] {
    request.body["input"].as_array().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [PROVIDER-01, PROVIDER-04, HANDOFF-02]
async fn real_codex_websocket_reuses_continuations_and_resets_on_restart_and_handoff() {
    let started = Instant::now();
    let mut provider = ControlledCodexProvider::start(64).unwrap();
    let mut agent = RealAgent::new().unwrap();
    agent
        .install_profile("codex", profile(provider.base_url(), 872_000, 128_000))
        .unwrap();
    agent
        .install_profile("codex-handoff", profile(provider.base_url(), 20_000, 1_000))
        .unwrap();

    let parallel_session = agent
        .create_configured_session(
            selection("codex"),
            Some("Use the requested tools, then finish.".into()),
        )
        .await
        .unwrap();
    let workspace = agent.workspace(&parallel_session).unwrap();
    std::fs::write(workspace.join("README.md"), "websocket continuation\n").unwrap();
    std::fs::write(workspace.join("NOTES.md"), "parallel tool call\n").unwrap();
    agent
        .send_mail(
            &parallel_session,
            "read README.md and NOTES.md, run the command, then finish",
        )
        .await
        .unwrap();

    let first = provider.request().await;
    assert_eq!(provider.connection_count(), 1);
    assert_eq!(
        first.headers.get("authorization").map(String::as_str),
        Some("Bearer codex-secret")
    );
    assert_eq!(
        first.headers.get("openai-beta").map(String::as_str),
        Some("responses_websockets=2026-02-06")
    );
    assert_eq!(
        first.headers.get("session-id").map(String::as_str),
        Some(parallel_session.as_str())
    );
    assert_eq!(
        first.headers.get("x-codex-fixture").map(String::as_str),
        Some("present")
    );
    assert_eq!(first.body["type"], "response.create");
    assert_eq!(first.body["stream"], true);
    assert_eq!(first.body["service_tier"], "priority");
    assert_eq!(first.body["parallel_tool_calls"], true);
    assert_eq!(first.body["reasoning"]["effort"], "max");
    assert_eq!(first.body["prompt_cache_key"], parallel_session);
    assert!(first.body.get("previous_response_id").is_none());
    assert_eq!(request_input(&first)[0]["type"], "additional_tools");
    assert_eq!(
        request_input(&first)[0]["tools"][0]["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request_input(&first)[0]["tools"][0]["tools"][0]["name"],
        "call"
    );

    let raw_items = vec![
        reasoning_item(),
        call_item(
            "fc_readme",
            "call_readme",
            "file.read",
            json!({"path": "README.md"}),
        ),
        call_item(
            "fc_notes",
            "call_notes",
            "file.read",
            json!({"path": "NOTES.md"}),
        ),
        call_item(
            "fc_shell",
            "call_shell",
            "shell.run",
            json!({
                "command": "printf started > ping-started; sleep 0.4; printf finished > ping-finished; printf done",
                "timeout_seconds": 5
            }),
        ),
    ];
    first
        .respond("resp_parallel", &raw_items, 200, 60)
        .await
        .unwrap();
    wait_for_file(&workspace.join("ping-started")).await;
    assert_eq!(
        first.ping(b"ping-during-tool").await.unwrap(),
        b"ping-during-tool"
    );

    let continuation = provider.request().await;
    assert_eq!(continuation.connection_id, first.connection_id);
    assert_eq!(continuation.body["previous_response_id"], "resp_parallel");
    let continuation_ids = request_input(&continuation)
        .iter()
        .map(|item| item["call_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        continuation_ids,
        ["call_readme", "call_notes", "call_shell"]
    );
    assert!(!continuation.body.to_string().contains("encrypted-parallel"));
    finish(continuation, "parallel").await;
    agent
        .wait_for_state(&parallel_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;

    let empty_session = agent
        .create_configured_session(selection("codex"), None)
        .await
        .unwrap();
    agent
        .send_mail(&empty_session, "continue until explicit end")
        .await
        .unwrap();
    let before_empty = provider.request().await;
    before_empty
        .respond("resp_empty", &[], 10, 0)
        .await
        .unwrap();
    let after_empty = provider.request().await;
    assert_eq!(after_empty.connection_id, before_empty.connection_id);
    assert_eq!(after_empty.body["previous_response_id"], "resp_empty");
    assert!(request_input(&after_empty).is_empty());
    finish(after_empty, "empty").await;
    agent
        .wait_for_state(&empty_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    assert!(agent
        .history(&empty_session, None, 200)
        .unwrap()
        .iter()
        .any(|event| {
            matches!(
                &event.event,
                SessionEvent::StepCompleted {
                    assistant_text,
                    provider_context: Some(context),
                    ..
                } if assistant_text.is_empty() && context.output_items.is_empty()
            )
        }));

    let failed_response_session = agent
        .create_configured_session(selection("codex"), None)
        .await
        .unwrap();
    agent
        .send_mail(
            &failed_response_session,
            "recover after the controlled provider error",
        )
        .await
        .unwrap();
    let failed_response = provider.request().await;
    let failed_connection = failed_response.connection_id;
    failed_response
        .fail(
            429,
            "rate_limit_exceeded",
            "slow down for controlled recovery",
            "req_controlled_failure",
        )
        .await
        .unwrap();
    let recovered_response = provider.request().await;
    assert_ne!(recovered_response.connection_id, failed_connection);
    assert!(recovered_response
        .body
        .to_string()
        .contains("slow down for controlled recovery"));
    finish(recovered_response, "provider_failure").await;
    agent
        .wait_for_state(&failed_response_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    assert!(agent
        .history(&failed_response_session, None, 200)
        .unwrap()
        .iter()
        .any(|event| matches!(
            &event.event,
            SessionEvent::StepFailed { error, .. }
                if error.status_code == Some(429)
                    && error.provider_code.as_deref() == Some("rate_limit_exceeded")
                    && error.request_id.as_deref() == Some("req_controlled_failure")
        )));

    let mut non_streaming_profile = profile(provider.base_url(), 872_000, 128_000);
    non_streaming_profile["models"][0]["streaming"] = json!(false);
    agent
        .install_profile("codex-non-streaming", non_streaming_profile)
        .unwrap();
    let repaired_selection_session = agent
        .create_configured_session(selection("codex-non-streaming"), None)
        .await
        .unwrap();
    agent
        .send_mail(
            &repaired_selection_session,
            "continue after the profile is repaired",
        )
        .await
        .unwrap();
    agent
        .wait_for_state(&repaired_selection_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Failed)
        })
        .await;
    assert!(agent
        .history(&repaired_selection_session, None, 200)
        .unwrap()
        .iter()
        .any(|event| matches!(
            &event.event,
            SessionEvent::StepFailed { error, .. }
                if error.provider_code.as_deref() == Some("invalid_selection")
        )));
    agent
        .install_profile(
            "codex-non-streaming",
            profile(provider.base_url(), 872_000, 128_000),
        )
        .unwrap();
    agent
        .send_mail(&repaired_selection_session, "the profile is repaired now")
        .await
        .unwrap();
    let repaired = provider.request().await;
    assert!(repaired
        .body
        .to_string()
        .contains("The provider selection is invalid"));
    assert!(repaired
        .body
        .to_string()
        .contains("the profile is repaired now"));
    finish(repaired, "profile_repaired").await;
    agent
        .wait_for_state(&repaired_selection_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;

    let restart_session = agent
        .create_configured_session(selection("codex"), None)
        .await
        .unwrap();
    std::fs::write(
        agent.workspace(&restart_session).unwrap().join("README.md"),
        "durable restart\n",
    )
    .unwrap();
    agent
        .send_mail(&restart_session, "read README before restart")
        .await
        .unwrap();
    let before_restart = provider.request().await;
    let restart_raw = vec![
        json!({
            "id": "rs_restart",
            "type": "reasoning",
            "status": "completed",
            "encrypted_content": "encrypted-restart",
            "summary": [{"type": "summary_text", "text": "Inspect the file."}],
            "provider_extension": "preserve-me"
        }),
        call_item(
            "fc_restart",
            "call_restart",
            "file.read",
            json!({"path": "README.md"}),
        ),
    ];
    before_restart
        .respond("resp_restart", &restart_raw, 200, 60)
        .await
        .unwrap();
    let pending_before_restart = provider.request().await;
    assert_eq!(
        pending_before_restart.body["previous_response_id"],
        "resp_restart"
    );
    let old_connection = pending_before_restart.connection_id;
    agent.stop_agent().await;
    drop(pending_before_restart);
    agent.start_agent().await.unwrap();
    let restarted = provider.request().await;
    assert_ne!(restarted.connection_id, old_connection);
    assert!(restarted.body.get("previous_response_id").is_none());
    let restarted_input = request_input(&restarted);
    let raw_start = restarted_input
        .iter()
        .position(|item| item["id"] == "rs_restart")
        .unwrap();
    assert_eq!(
        &restarted_input[raw_start..raw_start + restart_raw.len()],
        restart_raw.as_slice()
    );
    assert!(restarted_input.iter().any(|item| {
        item["type"] == "function_call_output" && item["call_id"] == "call_restart"
    }));
    finish(restarted, "restart").await;
    agent
        .wait_for_state(&restart_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;

    let handoff_session = agent
        .create_configured_session(selection("codex-handoff"), None)
        .await
        .unwrap();
    agent
        .send_mail(&handoff_session, "continue the durable task")
        .await
        .unwrap();
    let before_handoff = provider.request().await;
    before_handoff
        .respond(
            "resp_before_handoff",
            &[text_item("msg_before_handoff", "I will continue.")],
            18_900,
            200,
        )
        .await
        .unwrap();
    let handoff_request = provider.request().await;
    assert_eq!(handoff_request.connection_id, before_handoff.connection_id);
    assert_eq!(
        handoff_request.body["previous_response_id"],
        "resp_before_handoff"
    );
    assert!(handoff_request
        .body
        .to_string()
        .contains("successor-facing context handoff"));
    handoff_request
        .respond(
            "resp_handoff",
            &[call_item(
                "fc_handoff",
                "call_handoff",
                "handoff",
                json!({"document": "Goal: finish the durable task in generation two."}),
            )],
            19_300,
            120,
        )
        .await
        .unwrap();
    let generation_two = provider.request().await;
    assert_ne!(generation_two.connection_id, handoff_request.connection_id);
    assert!(generation_two.body.get("previous_response_id").is_none());
    assert!(generation_two
        .body
        .to_string()
        .contains("Goal: finish the durable task in generation two."));
    finish(generation_two, "handoff").await;
    let handoff_state = agent
        .wait_for_state(&handoff_session, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    assert_eq!(handoff_state.generation.number, 2);
    assert!(agent
        .history(&handoff_session, None, 200)
        .unwrap()
        .iter()
        .any(|event| {
            matches!(
                &event.event,
                SessionEvent::HandoffApplied {
                    generation: 2,
                    document: Some(document),
                    ..
                } if document.contains("generation two")
            )
        }));

    agent.shutdown().await;
    provider.shutdown().await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Codex WebSocket lifecycle took {:?}",
        started.elapsed()
    );
}
