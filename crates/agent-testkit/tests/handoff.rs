use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use zork_agent::session::events::{Purpose, SessionEvent, StepInterruptionReason, TurnOutcome};
use zork_agent::session::model::{ModelOutcome, ModelTokenUsage};
use zork_agent::session::service::ServiceOptions;
use zork_agent::session::tools::{ToolContract, ToolVersion, PROVIDER_CALL_NAME};
use zork_agent::session::wire::{ProviderToolCall, SessionSelection, TranscriptRole};
use zork_agent_testkit::model::ModelRelease;
use zork_agent_testkit::TestWorld;

fn selection() -> SessionSelection {
    SessionSelection {
        profile_id: "test-profile".into(),
        model: "test-model".into(),
        thinking: "medium".into(),
    }
}

fn outcome(
    text: &str,
    calls: impl IntoIterator<Item = (&'static str, &'static str, serde_json::Value)>,
    input_tokens: u64,
) -> ModelOutcome {
    ModelOutcome {
        text: text.into(),
        tool_calls: calls
            .into_iter()
            .map(|(provider_call_id, tool, arguments)| ProviderToolCall {
                tool_call_id: provider_call_id.into(),
                tool_name: PROVIDER_CALL_NAME.into(),
                arguments: json!({"tool": tool, "arguments": arguments}),
            })
            .collect(),
        provider_context: None,
        usage: Some(ModelTokenUsage {
            input_tokens,
            cached_input_tokens: Some(input_tokens.saturating_sub(1)),
            output_tokens: 1,
            output_reasoning_tokens: None,
            output_text_tokens: Some(1),
        }),
        provider_input: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [PROVIDER-02]
async fn usage_from_an_old_selection_cannot_restore_its_token_anchor() {
    let mut world = TestWorld::new();
    let session_id = world
        .create_session(selection(), None, "/virtual/selection-anchor")
        .await
        .unwrap();
    world.send_mail(&session_id, "first request").await.unwrap();
    let old_request = world.request().await;

    let replacement = SessionSelection {
        profile_id: "replacement-profile".into(),
        model: "replacement-model".into(),
        thinking: "high".into(),
    };
    world
        .service_handle()
        .set_selection(&session_id, replacement.clone())
        .await
        .unwrap();
    old_request
        .respond(Ok(outcome("old selection reply", [], 50)))
        .unwrap();

    let next = world.request().await;
    assert_eq!(next.selection, replacement);
    assert!(world
        .state(&session_id)
        .await
        .unwrap()
        .token_anchor
        .is_none());
    drop(next);
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [HANDOFF-01, HANDOFF-02, PROVIDER-02, PROVIDER-04, PROJECTION-01]
async fn token_anchor_handoff_carries_live_tools_and_delivers_their_result_as_a_notification() {
    let mut options = ServiceOptions::default();
    options.runner.input_budget = Arc::new(|_| Some(100));
    options.runner.max_output_tokens = Arc::new(|_| Some(25));
    let mut world = TestWorld::with_options(options);
    let mut slow_tool = world
        .install_tool(ToolContract {
            name: "test.slow".into(),
            version: ToolVersion::new("test-1").unwrap(),
            initial_description: "Complete controlled work later.".into(),
            detailed_description: "The test controller decides when this work completes.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            }),
        })
        .unwrap();
    let session_id = world
        .create_session(selection(), None, "/virtual/handoff")
        .await
        .unwrap();

    world
        .send_mail(&session_id, "start durable work")
        .await
        .unwrap();
    let first = world.request().await;
    assert_eq!(first.generation, 1);
    assert_eq!(first.max_output_tokens, Some(25));
    let first_prefix = first.transcript.clone();
    first
        .respond(Ok(outcome(
            "The work has started.",
            [("provider-slow-1", "test.slow", json!({"value": "one"}))],
            100,
        )))
        .unwrap();
    let pending_slow = slow_tool.request().await;
    world
        .wait_for_state(&session_id, |state| state.auto_wait.is_some())
        .await;

    world
        .send_mail(&session_id, "new input while work remains")
        .await
        .unwrap();
    let first_handoff = world.request().await;
    assert_eq!(first_handoff.generation, 1);
    assert!(first_handoff
        .transcript
        .starts_with(first_prefix.as_slice()));
    assert_eq!(first_handoff.tools.len(), 1);
    assert_eq!(first_handoff.tools[0].name, PROVIDER_CALL_NAME);
    assert!(first_handoff
        .transcript
        .iter()
        .any(|message| { message.content.contains("successor-facing context handoff") }));
    let first_handoff_prefix = first_handoff.transcript.clone();
    first_handoff
        .respond(Ok(outcome("I am preparing the handoff.", [], 110)))
        .unwrap();

    let second_handoff = world.request().await;
    assert_eq!(second_handoff.generation, 1);
    assert!(second_handoff
        .transcript
        .starts_with(first_handoff_prefix.as_slice()));
    assert!(second_handoff.transcript.iter().any(|message| {
        message.role == TranscriptRole::Assistant
            && message.content.as_ref() == "I am preparing the handoff."
    }));
    second_handoff
        .respond(Ok(outcome(
            "",
            [(
                "provider-handoff-1",
                "handoff",
                json!({"document": "Goal: finish the durable work and report the late result."}),
            )],
            120,
        )))
        .unwrap();

    let generation_two = world.request().await;
    assert_eq!(generation_two.generation, 2);
    assert!(generation_two.transcript.iter().any(|message| {
        message
            .content
            .contains("Goal: finish the durable work and report the late result.")
    }));
    assert!(generation_two.transcript.iter().any(|message| {
        message
            .content
            .contains("These tool invocations were unfinished at handoff")
            && message.content.contains("test.slow")
    }));
    assert!(generation_two
        .transcript
        .iter()
        .all(|message| message.tool_call_id.as_deref() != Some("provider-slow-1")));

    pending_slow
        .succeed("late work completed", json!({"value": "one"}))
        .unwrap();
    generation_two
        .respond(Ok(outcome("I received the successor context.", [], 10)))
        .unwrap();

    let after_late_result = world.request().await;
    assert_eq!(after_late_result.generation, 2);
    assert!(after_late_result.transcript.iter().any(|message| {
        message
            .content
            .contains("A previously unfinished tool invocation has now returned")
            && message.content.contains("late work completed")
    }));
    assert!(after_late_result
        .transcript
        .iter()
        .all(|message| message.tool_call_id.as_deref() != Some("provider-slow-1")));
    after_late_result
        .respond_call("provider-end-1", "end", json!({}))
        .unwrap();

    let state = world
        .wait_for_state(&session_id, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    assert_eq!(state.generation.number, 2);
    let events = world.events(&session_id);
    assert!(events.iter().any(|event| matches!(
        &event.event,
        SessionEvent::StepStarted {
            purpose: Purpose::Handoff,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        SessionEvent::HandoffApplied {
            generation: 2,
            document: Some(document),
            carried_tools,
            ..
        } if document.contains("finish the durable work")
            && carried_tools.iter().any(|tool| tool.tool == "test.slow")
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event.event, SessionEvent::HandoffFailed { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.event, SessionEvent::Snapshot { .. })));
    assert!(world.model_releases().contains(&ModelRelease::Generation {
        session_id: session_id.clone(),
        generation: 1,
    }));
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [HANDOFF-03, PROJECTION-03]
async fn exhausted_handoff_retries_create_a_successor_with_only_the_document_missing() {
    let mut options = ServiceOptions::default();
    options.runner.input_budget = Arc::new(|_| Some(100));
    options.runner.max_output_tokens = Arc::new(|_| Some(25));
    options.runner.provider_retry_limit = 2;
    options.runner.provider_retry_base = Duration::from_millis(1);
    options.runner.provider_retry_max = Duration::from_millis(1);
    let mut world = TestWorld::with_options(options);
    let mut slow = world
        .install_tool(ToolContract {
            name: "test.handoff-pending".into(),
            version: ToolVersion::new("test-1").unwrap(),
            initial_description: "Keep controlled work pending across handoff.".into(),
            detailed_description: "The controller completes this work later.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            }),
        })
        .unwrap();
    let session_id = world
        .create_session(selection(), None, "/virtual/handoff-exhausted")
        .await
        .unwrap();

    world
        .send_mail(&session_id, "start durable work")
        .await
        .unwrap();
    request_with_timeout(&mut world, "initial conversation")
        .await
        .respond(Ok(outcome(
            "Work is still running.",
            [(
                "provider-pending",
                "test.handoff-pending",
                json!({"value": "preserve"}),
            )],
            100,
        )))
        .unwrap();
    let pending = slow.request().await;
    let pending_id = pending.context.invocation_id.clone();
    world
        .wait_for_state(&session_id, |state| state.auto_wait.is_some())
        .await;
    world
        .send_mail(&session_id, "input that the successor must receive")
        .await
        .unwrap();

    for attempt in 1..=2 {
        request_with_timeout(&mut world, &format!("handoff attempt {attempt}"))
            .await
            .fail_provider(
                "controlled.handoff",
                true,
                format!("handoff provider failure {attempt}"),
            )
            .unwrap();
        if attempt == 1 {
            world
                .wait_for_state(&session_id, |state| {
                    state
                        .active_turn
                        .as_ref()
                        .is_some_and(|turn| turn.consecutive_provider_failures == 1)
                })
                .await;
            wait_for_clock_deadline(&world, world.clock.current_ms() + 1).await;
            world.clock.advance(Duration::from_millis(1));
        }
    }

    let successor = request_with_timeout(&mut world, "successor generation").await;
    assert_eq!(successor.generation, 2);
    assert!(successor.transcript.iter().any(|message| {
        message
            .content
            .contains("Context handoff completed without a handoff document")
            && message.content.contains("2 handoff steps")
    }));
    assert!(successor
        .transcript
        .iter()
        .any(|message| { message.content.contains("handoff provider failure 2") }));
    assert!(successor
        .transcript
        .iter()
        .any(|message| { message.content.as_ref() == "input that the successor must receive" }));
    assert!(successor.transcript.iter().any(|message| {
        message
            .content
            .contains("These tool invocations were unfinished at handoff")
            && message.content.contains("test.handoff-pending")
    }));
    let state = world.state(&session_id).await.unwrap();
    assert_eq!(state.generation.number, 2);
    assert!(state.generation.handoff_document.is_none());
    assert_eq!(state.selection.as_ref().unwrap().model, "test-model");
    assert!(state.pending_tools.contains_key(&pending_id));

    let events = world.events(&session_id);
    let failed_index = events
        .iter()
        .position(|event| matches!(event.event, SessionEvent::HandoffFailed { .. }))
        .unwrap();
    let applied_index = events
        .iter()
        .position(|event| {
            matches!(
                event.event,
                SessionEvent::HandoffApplied {
                    generation: 2,
                    document: None,
                    ..
                }
            )
        })
        .unwrap();
    assert_eq!(applied_index, failed_index + 1);
    assert_eq!(events[failed_index].batch_count, 2);
    assert_eq!(events[failed_index].batch_index, 0);
    assert_eq!(events[applied_index].batch_count, 2);
    assert_eq!(events[applied_index].batch_index, 1);

    pending
        .succeed("preserved work completed", json!({"value": "preserve"}))
        .unwrap();
    successor
        .respond_text("I will use the recovered result.")
        .unwrap();
    let final_request = request_with_timeout(&mut world, "late-result delivery").await;
    assert!(final_request
        .transcript
        .iter()
        .any(|message| { message.content.contains("preserved work completed") }));
    final_request
        .respond_call("provider-handoff-failure-end", "end", json!({}))
        .unwrap();
    world
        .wait_for_state(&session_id, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [HANDOFF-01, RETRY-01]
async fn handoff_survives_an_interrupted_request_a_provider_retry_and_new_mail() {
    let mut options = ServiceOptions::default();
    options.runner.input_budget = Arc::new(|_| Some(100));
    options.runner.max_output_tokens = Arc::new(|_| Some(25));
    options.runner.provider_retry_base = Duration::from_millis(1);
    options.runner.provider_retry_max = Duration::from_millis(1);
    let mut world = TestWorld::with_options(options);
    let session_id = world
        .create_session(selection(), None, "/virtual/handoff-recovery")
        .await
        .unwrap();

    world
        .send_mail(&session_id, "preserve the durable task")
        .await
        .unwrap();
    world
        .request()
        .await
        .respond(Ok(outcome(
            "The durable task has enough context to require handoff.",
            [],
            100,
        )))
        .unwrap();
    let interrupted_handoff = world.request().await;
    assert!(interrupted_handoff
        .transcript
        .iter()
        .any(|message| message.content.contains("successor-facing context handoff")));

    world
        .send_mail(&session_id, "mail received while handoff was in flight")
        .await
        .unwrap();
    world.restart().await.unwrap();
    drop(interrupted_handoff);

    let failed_handoff = world.request().await;
    assert!(failed_handoff.transcript.iter().any(|message| {
        message
            .content
            .contains("was interrupted during runtime recovery")
    }));
    failed_handoff
        .fail_provider(
            "controlled.handoff",
            true,
            "temporary handoff provider outage",
        )
        .unwrap();
    world
        .wait_for_state(&session_id, |state| {
            state.last_step_failure.as_ref().is_some_and(|failure| {
                failure.purpose == Purpose::Handoff
                    && failure.error.message == "temporary handoff provider outage"
            })
        })
        .await;
    wait_for_clock_timer(&world).await;
    world.clock.advance(Duration::from_millis(1));

    let recovered_handoff = world.request().await;
    assert!(recovered_handoff.transcript.iter().any(|message| {
        message
            .content
            .contains("temporary handoff provider outage")
    }));
    recovered_handoff
        .respond_call(
            "provider-handoff-recovered",
            "handoff",
            json!({
                "document": "Goal: finish the durable task after recovery."
            }),
        )
        .unwrap();

    let generation_two = world.request().await;
    assert_eq!(generation_two.generation, 2);
    assert!(generation_two.transcript.iter().any(|message| {
        message
            .content
            .contains("Goal: finish the durable task after recovery.")
    }));
    assert!(generation_two.transcript.iter().any(|message| {
        message.content.as_ref() == "mail received while handoff was in flight"
    }));
    generation_two
        .respond_call("provider-end-after-handoff-recovery", "end", json!({}))
        .unwrap();

    let state = world
        .wait_for_state(&session_id, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    assert_eq!(state.generation.number, 2);
    let events = world.events(&session_id);
    assert!(events.iter().any(|event| matches!(
        event.event,
        SessionEvent::StepInterrupted {
            reason: StepInterruptionReason::Recovery,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        &event.event,
        SessionEvent::StepFailed { error, .. }
            if error.message == "temporary handoff provider outage"
    )));
    assert!(events.iter().any(|event| matches!(
        event.event,
        SessionEvent::HandoffApplied { generation: 2, .. }
    )));
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
// Contract: docs/zork-agent-architecture.md [SNAPSHOT-02, HANDOFF-02]
async fn handoff_snapshot_size_is_independent_of_the_sealed_transcript_length() {
    let short = snapshot_size_after_history(4).await;
    let long = snapshot_size_after_history(200).await;
    assert!(
        long <= short + 512,
        "long sealed history grew snapshot from {short} to {long} bytes"
    );
}

async fn snapshot_size_after_history(step_count: usize) -> usize {
    let budget = Arc::new(AtomicU64::new(u64::MAX));
    let mut options = ServiceOptions::default();
    let active_budget = budget.clone();
    options.runner.input_budget = Arc::new(move |_| Some(active_budget.load(Ordering::Relaxed)));
    let mut world = TestWorld::with_options(options);
    let session_id = world
        .create_session(selection(), None, "/virtual/bounded-snapshot")
        .await
        .unwrap();
    world
        .send_mail(&session_id, "build sealed history")
        .await
        .unwrap();

    for index in 0..step_count {
        let request = request_with_timeout(&mut world, &format!("history step {index}")).await;
        if index + 1 == step_count {
            budget.store(1, Ordering::Relaxed);
        }
        request
            .respond_text(format!("sealed-transcript-entry-{index:04}"))
            .unwrap();
    }

    let handoff = request_with_timeout(&mut world, "bounded snapshot handoff").await;
    assert!(handoff
        .transcript
        .iter()
        .any(|message| message.content.contains("successor-facing context handoff")));
    handoff
        .respond_call(
            "provider-bounded-handoff",
            "handoff",
            json!({"document": "continue with the bounded successor state"}),
        )
        .unwrap();
    let successor = request_with_timeout(&mut world, "bounded snapshot successor").await;
    assert_eq!(successor.generation, 2);

    let snapshots = world
        .events(&session_id)
        .into_iter()
        .filter_map(|event| match event.event {
            SessionEvent::Snapshot { state, .. } => Some(state),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 1);
    let encoded = serde_json::to_vec(&snapshots[0]).unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("sealed-transcript-entry"));

    successor
        .respond_call("provider-bounded-end", "end", json!({}))
        .unwrap();
    world
        .wait_for_state(&session_id, |state| {
            state.last_turn_outcome == Some(TurnOutcome::Finished)
        })
        .await;
    world.shutdown().await;
    encoded.len()
}

async fn wait_for_clock_timer(world: &TestWorld) {
    for _ in 0..512 {
        if world.clock.pending_timer_count() > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("zork-agent did not arm the expected virtual timer");
}

async fn wait_for_clock_deadline(world: &TestWorld, expected_deadline_ms: i64) {
    for _ in 0..512 {
        if world
            .clock
            .pending_deadlines()
            .contains(&expected_deadline_ms)
        {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("zork-agent did not arm virtual deadline {expected_deadline_ms}");
}

async fn request_with_timeout(
    world: &mut TestWorld,
    stage: &str,
) -> zork_agent_testkit::PendingModelRequest {
    tokio::time::timeout(Duration::from_secs(1), world.request())
        .await
        .unwrap_or_else(|_| panic!("zork-agent did not issue the expected request for {stage}"))
}
