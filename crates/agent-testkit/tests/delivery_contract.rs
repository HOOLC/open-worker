use serde_json::json;
use std::collections::BTreeSet;
use zork_agent::session::{
    decision::{decide, Decision, DecisionWorld},
    events::{
        AutoWaitEndReason, Input, SessionEvent, ToolDelivery, ToolDeliveryMode, ToolOutcome,
        ToolResultData,
    },
    tools::{ToolContract, ToolExecution, ToolKnowledge, ToolVersion},
    wire::SessionSelection,
};
use zork_agent_testkit::TestWorld;
fn tool(name: &str, version: &str) -> ToolContract {
    ToolContract {
        name: name.into(),
        version: ToolVersion::new(version).unwrap(),
        initial_description: name.into(),
        detailed_description: name.into(),
        input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
    }
}
async fn session(w: &TestWorld) -> String {
    w.create_session(
        SessionSelection {
            profile_id: "test-profile".into(),
            model: "test-model".into(),
            thinking: "medium".into(),
        },
        None,
        "/virtual/original-design",
    )
    .await
    .unwrap()
}

// User clarification: completed results may merge until the next request is frozen.
#[tokio::test(flavor = "multi_thread")]
async fn results_before_the_next_request_are_merged_after_wait_ends() {
    let mut w = TestWorld::new();
    let mut slow = w.install_tool(tool("test.slow", "v1")).unwrap();
    let id = session(&w).await;
    w.send_mail(&id, "start").await.unwrap();
    w.request()
        .await
        .respond_call("original-call", "test.slow", json!({}))
        .unwrap();
    let pending = slow.request().await;
    let mut s = w.state(&id).await.unwrap();
    let wait = s.auto_wait.clone().unwrap();
    let invocation = s
        .pending(&pending.context.invocation_id)
        .unwrap()
        .invocation
        .clone();
    s.apply(
        &SessionEvent::InputAppended {
            input: Input {
                position: None,
                wake: true,
                input_id: "new-input".into(),
                request_id: None,
                content: "interrupt".into(),
                received_at_ms: 20,
            },
        },
        &w.tools,
    )
    .unwrap();
    s.apply(
        &SessionEvent::AutoWaitEnded {
            step_id: wait.step_id,
            reason: AutoWaitEndReason::NewInput,
            ended_at_ms: 20,
        },
        &w.tools,
    )
    .unwrap();
    s.apply(
        &SessionEvent::ToolResult {
            result: ToolResultData {
                images: Vec::new(),
                invocation_id: invocation.invocation_id.clone(),
                tool: invocation.tool.clone(),
                outcome: ToolOutcome::Succeeded,
                data: json!({"message":"completed after wait ended"}),
                result_schema_version: 1,
                knowledge: None,
                finished_at_ms: 21,
            },
        },
        &w.tools,
    )
    .unwrap();
    let deliveries = s.planned_deliveries(true);
    assert!(
        deliveries.iter().any(|d| matches!(
            d,
            ToolDelivery::Result {
                mode: ToolDeliveryMode::Direct,
                ..
            }
        )),
        "results completed before the next request must be merged as Direct"
    );
    drop(pending);
    w.shutdown().await;
}

// Original decisions 92, 93, 98, 100: result knowledge becomes known when delivered.
#[tokio::test(flavor = "multi_thread")]
async fn undelivered_help_result_cannot_upgrade_the_models_known_tool_version() {
    let mut w = TestWorld::new();
    let _old = w.install_tool(tool("test.target", "v1")).unwrap();
    let mut help = w.install_tool(tool("test.help", "v1")).unwrap();
    let id = session(&w).await;
    w.send_mail(&id, "look up current tool usage")
        .await
        .unwrap();
    w.request()
        .await
        .respond_call("help-call", "test.help", json!({}))
        .unwrap();
    let pending = help.request().await;
    w.send_mail(&id, "new user instruction").await.unwrap();
    let in_flight = w.request().await;
    assert!(!in_flight
        .transcript
        .iter()
        .any(|m| m.content.contains("v2")));
    let _new = w.install_tool(tool("test.target", "v2")).unwrap();
    pending
        .respond(ToolExecution {
            images: Vec::new(),
            outcome: ToolOutcome::Succeeded,
            data: json!({"usage":"v2 usage"}),
            result_schema_version: 1,
            knowledge: Some(ToolKnowledge::Current {
                name: "test.target".into(),
                version: ToolVersion::new("v2").unwrap(),
            }),
        })
        .unwrap();
    w.wait_for_state(&id, |s| {
        s.pending_tools
            .values()
            .any(|p| p.invocation.tool == "test.help" && p.result.is_some())
    })
    .await;
    let state = w.state(&id).await.unwrap();
    assert_eq!(
        state.known_tools["test.target"],
        ToolVersion::new("v1").unwrap(),
        "the model has not received v2 knowledge, but state already trusts it as known"
    );
    in_flight.respond_text("deliver the lookup result").unwrap();
    let delivered = w.request().await;
    assert_eq!(
        w.state(&id).await.unwrap().known_tools["test.target"],
        ToolVersion::new("v2").unwrap()
    );
    assert!(delivered
        .transcript
        .iter()
        .any(|m| m.content.contains("v2 usage")));
    assert!(
        !delivered
            .transcript
            .iter()
            .any(|m| m.content.contains("Tool test.target was updated")),
        "the delivered result already reports this version; no duplicate update notice"
    );
    delivered.respond_text("done").unwrap();
    w.wait_for_state(&id, |s| {
        s.last_turn_outcome == Some(zork_agent::session::events::TurnOutcome::Finished)
    })
    .await;
    w.shutdown().await;
}

// Original decisions 71, 73: acknowledge_outstanding is valid after showing the list.
#[tokio::test(flavor = "multi_thread")]
async fn acknowledge_end_cannot_skip_the_first_outstanding_disclosure() {
    let mut w = TestWorld::new();
    let mut slow = w.install_tool(tool("test.slow", "v1")).unwrap();
    let id = session(&w).await;
    w.send_mail(&id, "work").await.unwrap();
    w.request()
        .await
        .respond_calls([
            ("slow", "test.slow", json!({})),
            ("end", "end", json!({"acknowledge_outstanding":true})),
        ])
        .unwrap();
    let pending = slow.request().await;
    w.wait_for_state(&id, |s| {
        s.pending_tools
            .values()
            .any(|p| p.invocation.tool == "end" && p.result.is_some())
    })
    .await;
    let mut s = w.state(&id).await.unwrap();
    let wait = s.auto_wait.clone().unwrap();
    let ended_at = wait.deadline_ms;
    s.apply(
        &SessionEvent::AutoWaitEnded {
            step_id: wait.step_id,
            reason: AutoWaitEndReason::TimedOut,
            ended_at_ms: ended_at,
        },
        &w.tools,
    )
    .unwrap();
    let world = DecisionWorld {
        now_ms: ended_at,
        live_tools: BTreeSet::from([pending.context.invocation_id.clone()]),
        tool_changes: vec![],
        outstanding: s.outstanding(&w.tools),
        estimated_input_tokens: None,
        input_budget: None,
        provider_retry_limit: 10,
        context_attempt_limit: 10,
    };
    assert!(
        matches!(decide(&s, &world), Decision::StartStep { .. }),
        "end was accepted before any outstanding list reached the model"
    );
    drop(pending);
    w.shutdown().await;
}

// Original decision 53: schema describes; the tool's own parser is the execution boundary.
#[tokio::test(flavor = "multi_thread")]
async fn logical_tool_parameters_are_not_silently_rewritten_by_generic_schema_processing() {
    let mut w = TestWorld::new();
    let mut custom = w.install_tool(tool("test.parser", "v1")).unwrap();
    let id = session(&w).await;
    w.send_mail(&id, "parse args").await.unwrap();
    w.request()
        .await
        .respond_call(
            "parser-call",
            "test.parser",
            json!({"unrecognized":"must reach the tool parser"}),
        )
        .unwrap();
    let request = custom.request().await;
    assert_eq!(
        request.arguments,
        json!({"unrecognized":"must reach the tool parser"}),
        "generic executor removed an argument before the tool parser could handle or reject it"
    );
    drop(request);
    w.shutdown().await;
}
