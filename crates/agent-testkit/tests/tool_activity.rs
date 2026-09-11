use serde_json::json;
use zork_agent::session::events::{SessionEvent, TurnOutcome};
use zork_agent::session::wire::SessionSelection;
use zork_agent::session::{model::ModelOutcome, wire::ProviderToolCall};
use zork_agent_testkit::TestWorld;

#[tokio::test(flavor = "multi_thread")]
async fn missing_descriptions_do_not_execute_and_a_corrected_call_runs_once() {
    let mut world = TestWorld::new();
    let id = world
        .create_session(
            SessionSelection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            "/virtual/activity-validation",
        )
        .await
        .unwrap();
    world.send_mail(&id, "write report").await.unwrap();
    world.request().await.respond(Ok(ModelOutcome {
        text: String::new(), tool_calls: vec![ProviderToolCall {
            tool_call_id: "missing".into(), tool_name: "call".into(),
            arguments: json!({"tool":"file.write","arguments":{"path":"report.md","content":"report"}}),
        }], provider_context: None, usage: None, provider_input: None,
    })).unwrap();
    let correction = world.request().await;
    assert!(!world.files.exists("/virtual/activity-validation/report.md"));
    assert!(correction.transcript.iter().any(|message| message.is_error
        && message.content.contains("top-level action")
        && message.content.contains("No tool was executed")));
    correction
        .respond_call(
            "corrected",
            "file.write",
            json!({"path":"report.md","content":"report"}),
        )
        .unwrap();
    let next = world.request().await;
    assert_eq!(
        world
            .files
            .read_text("/virtual/activity-validation/report.md")
            .as_deref(),
        Some("report")
    );
    next.respond_text("done").unwrap();
    world
        .wait_for_state(&id, |s| s.last_turn_outcome == Some(TurnOutcome::Finished))
        .await;
    let successful_writes = world.events(&id).iter().filter(|e| matches!(&e.event, SessionEvent::ToolResult { result } if result.tool == "file.write" && result.outcome == zork_agent::session::events::ToolOutcome::Succeeded)).count();
    assert_eq!(successful_writes, 1);
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_activity_is_captured_before_execution_and_survives_replay() {
    let mut world = TestWorld::new();
    let id = world
        .create_session(
            SessionSelection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            "/virtual/activity",
        )
        .await
        .unwrap();
    world.send_mail(&id, "write a report").await.unwrap();
    world
        .request()
        .await
        .respond(Ok(ModelOutcome {
            text: String::new(),
            tool_calls: vec![ProviderToolCall {
                tool_call_id: "write".into(), tool_name: "call".into(),
                arguments: json!({"tool":"file.write","goal":"整理检查结果","action":"写入检查报告","arguments":{"path":"report.md","content":"private body"}}),
            }],
            provider_context: None, usage: None, provider_input: None,
        }))
        .unwrap();
    world.request().await.respond_text("done").unwrap();
    world
        .wait_for_state(&id, |s| s.last_turn_outcome == Some(TurnOutcome::Finished))
        .await;
    let captured = |world: &TestWorld| {
        world
            .events(&id)
            .into_iter()
            .find_map(|envelope| {
                let SessionEvent::StepCompleted { invocations, .. } = envelope.event else {
                    return None;
                };
                invocations
                    .into_iter()
                    .find(|call| call.tool == "file.write")
                    .and_then(|call| call.activity)
            })
            .unwrap()
    };
    let before = captured(&world);
    assert_eq!(before.labels["zh-CN"], "写入");
    assert_eq!(before.detail, "report.md");
    assert!(serde_json::to_value(&before).unwrap().get("goal").is_none());
    assert_eq!(before.action, "写入检查报告");
    assert!(!serde_json::to_string(&before)
        .unwrap()
        .contains("private body"));
    world.restart().await.unwrap();
    assert_eq!(captured(&world), before);
    world.shutdown().await;
}
