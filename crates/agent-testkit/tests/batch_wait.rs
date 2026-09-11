use serde_json::{json, Value};
use std::time::Duration;
use zork_agent::session::{
    model::ModelOutcome,
    wire::{ProviderToolCall, SessionSelection},
};
use zork_agent_testkit::TestWorld;

fn call(id: usize, tool: &str, arguments: Value, wait: Option<f64>) -> ProviderToolCall {
    let mut value = json!({"tool":tool,"action":"查看工具进度","arguments":arguments});
    if let Some(wait) = wait {
        value["wait"] = json!(wait);
    }
    ProviderToolCall {
        tool_call_id: format!("call-{id}"),
        tool_name: "call".into(),
        arguments: value,
    }
}
async fn start(world: &mut TestWorld, calls: Vec<ProviderToolCall>) -> String {
    let session = world
        .create_session(
            SessionSelection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            "/virtual/batch-wait",
        )
        .await
        .unwrap();
    world
        .send_mail(&session, "start work and check progress")
        .await
        .unwrap();
    world
        .request()
        .await
        .respond(Ok(ModelOutcome {
            text: String::new(),
            tool_calls: calls,
            provider_context: None,
            usage: None,
            provider_input: None,
        }))
        .unwrap();
    session
}
async fn armed(world: &TestWorld, deadline: i64) {
    tokio::time::timeout(
        Duration::from_secs(5),
        world.clock.wait_for_pending_deadline(deadline),
    )
    .await
    .expect("batch deadline armed");
}

#[tokio::test(flavor = "multi_thread")]
async fn shortest_wait_tool_wakes_with_a_long_command_in_either_result_order() {
    for seconds in [[2, 9], [9, 2]] {
        let mut world = TestWorld::new();
        let now = world.clock.current_ms();
        let session = start(
            &mut world,
            vec![
                call(0, "shell.run", json!({"command":"long"}), None),
                call(1, "wait", json!({"seconds":seconds[0]}), None),
                call(2, "wait", json!({"seconds":seconds[1]}), None),
            ],
        )
        .await;
        let process = world.process_request().await;
        world
            .wait_for_state(&session, |state| {
                state
                    .pending_tools
                    .values()
                    .filter(|p| p.invocation.tool == "wait" && p.result.is_some())
                    .count()
                    == 2
            })
            .await;
        armed(&world, now + 2000).await;
        world.clock.advance_to(now + 2000);
        let resumed = world.request().await;
        assert!(!process.was_killed());
        assert!(resumed
            .transcript
            .iter()
            .any(|m| m.content.contains("still unfinished") && m.content.contains("shell.run")));
        let state = world
            .wait_for_state(&session, |state| state.active_step.is_some())
            .await;
        assert!(state.wait_deadline.is_none());
        world.cancel(&session).await.unwrap();
        world.shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn call_wait_uses_shortest_explicit_estimate_including_zero_and_over_default() {
    for (waits, expected) in [
        ([Some(20.0), Some(2.0)], 2),
        ([Some(2.0), Some(20.0)], 2),
        ([None, Some(120.0)], 120),
        ([Some(0.0), None], 0),
        ([None, None], 60),
    ] {
        let mut world = TestWorld::new();
        let now = world.clock.current_ms();
        let session = start(
            &mut world,
            waits
                .into_iter()
                .enumerate()
                .map(|(id, wait)| {
                    call(
                        id,
                        "shell.run",
                        json!({"command":format!("long-{id}")}),
                        wait,
                    )
                })
                .collect(),
        )
        .await;
        let first = world.process_request().await;
        let second = world.process_request().await;
        if expected > 0 {
            armed(&world, now + expected * 1000).await;
            world.clock.advance_to(now + expected * 1000);
        }
        let resumed = world.request().await;
        assert!(!first.was_killed() && !second.was_killed());
        assert!(resumed
            .transcript
            .iter()
            .any(|m| m.content.contains("still unfinished")));
        assert_eq!(world.clock.current_ms(), now + expected * 1000);
        world.cancel(&session).await.unwrap();
        world.shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn completed_tools_resume_before_the_estimated_wait() {
    let mut world = TestWorld::new();
    let now = world.clock.current_ms();
    let session = start(
        &mut world,
        vec![call(0, "shell.run", json!({"command":"quick"}), Some(30.0))],
    )
    .await;
    world.process_request().await.succeed();
    let resumed = world.request().await;
    assert_eq!(world.clock.current_ms(), now);
    resumed.respond_text("done").unwrap();
    world
        .wait_for_state(&session, |state| state.active_turn.is_none())
        .await;
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn outer_wait_shortens_wait_tool_and_survives_restart_without_later_wait_resurrection() {
    let mut world = TestWorld::new();
    let now = world.clock.current_ms();
    let session = start(
        &mut world,
        vec![
            call(0, "wait", json!({"seconds":30}), Some(2.0)),
            call(1, "wait", json!({"seconds":10}), None),
        ],
    )
    .await;
    world
        .wait_for_state(&session, |state| {
            state.auto_wait.is_none()
                && state
                    .wait_deadline
                    .as_ref()
                    .is_some_and(|wait| wait.deadline_ms == now + 2000)
        })
        .await;
    world.restart().await.unwrap();
    armed(&world, now + 2000).await;
    world.clock.advance_to(now + 2000);
    let resumed = world.request().await;
    assert!(resumed
        .transcript
        .iter()
        .any(|m| m.content.contains("reached its deadline")));
    resumed.respond_text("done").unwrap();
    world
        .wait_for_state(&session, |state| state.active_turn.is_none())
        .await;
    world.clock.advance_to(now + 30000);
    let state = world
        .wait_for_state(&session, |state| state.active_turn.is_none())
        .await;
    assert!(state.wait_deadline.is_none());
    world.shutdown().await;
}
