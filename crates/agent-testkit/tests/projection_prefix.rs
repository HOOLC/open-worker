//! Contract: PROJECTION-01/02/03, WAIT-01, MAILBOX-01.
use serde_json::json;
use zork_agent::session::{
    events::{SessionEvent, ToolDelivery, ToolDeliveryMode, TurnOutcome},
    tools::{ToolContract, ToolVersion},
    wire::{SessionSelection, TranscriptRole},
};
use zork_agent_testkit::TestWorld;

fn tool(name: &str) -> ToolContract {
    ToolContract {
        name: name.into(),
        version: ToolVersion::new("test-1").unwrap(),
        initial_description: name.into(),
        detailed_description: name.into(),
        input_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mailbox_interrupt_keeps_placeholder_and_appends_late_result_without_rewriting_prefix() {
    for failed in [false, true] {
        let mut world = TestWorld::new();
        let mut fast = world.install_tool(tool("test.fast")).unwrap();
        let mut slow = world.install_tool(tool("test.slow")).unwrap();
        let id = world
            .create_session(
                SessionSelection {
                    profile_id: "test-profile".into(),
                    model: "test-model".into(),
                    thinking: "medium".into(),
                },
                None,
                "/virtual/projection-prefix",
            )
            .await
            .unwrap();
        world.send_mail(&id, "start").await.unwrap();
        let first = world.request().await;
        let original = first.transcript.clone();
        first
            .respond_calls([
                ("fast-call", "test.fast", json!({})),
                ("slow-call", "test.slow", json!({})),
            ])
            .unwrap();
        let fast = fast.request().await;
        let slow = slow.request().await;
        fast.succeed(json!({"message":"fast completed"})).unwrap();
        world
            .wait_for_state(&id, |s| {
                s.pending_tools
                    .values()
                    .any(|p| p.invocation.tool == "test.fast" && p.result.is_some())
            })
            .await;
        world
            .send_mail(&id, "new instruction while slow tool runs")
            .await
            .unwrap();
        let interrupted = world.request().await;
        let prefix = interrupted.transcript.clone();
        assert_eq!(&prefix[..original.len()], original.as_slice());
        let placeholder = prefix
            .iter()
            .position(|m| m.tool_call_id.as_deref() == Some("slow-call"))
            .unwrap();
        let mail = prefix
            .iter()
            .position(|m| m.content.as_ref() == "new instruction while slow tool runs")
            .unwrap();
        assert!(placeholder < mail);
        assert!(prefix[placeholder].content.contains("still unfinished"));
        assert!(prefix
            .iter()
            .any(|m| m.tool_call_id.as_deref() == Some("fast-call")
                && m.content.contains("fast completed")));
        if failed {
            slow.fail("late failure").unwrap();
        } else {
            slow.succeed(json!({"message":"late success"})).unwrap();
        }
        world
            .wait_for_state(&id, |s| {
                s.pending_tools
                    .values()
                    .any(|p| p.invocation.tool == "test.slow" && p.result.is_some())
            })
            .await;
        interrupted
            .respond_text("received the new instruction")
            .unwrap();
        let after = world.request().await;
        assert_eq!(
            &after.transcript[..prefix.len()],
            prefix.as_slice(),
            "previously sent prefix must not change"
        );
        let late = if failed {
            "late failure"
        } else {
            "late success"
        };
        let notification = after
            .transcript
            .iter()
            .position(|m| m.content.contains(late))
            .unwrap();
        assert!(notification >= prefix.len());
        assert_eq!(after.transcript[notification].role, TranscriptRole::Tool);
        assert!(after.transcript[notification - 1].runtime_generated);
        assert!(!after.transcript[notification].runtime_generated);
        assert!(after
            .transcript
            .iter()
            .filter(|message| message.role == TranscriptRole::User)
            .all(|message| !message.runtime_generated));
        assert!(after.transcript[notification]
            .tool_call_id
            .as_deref()
            .is_some_and(|id| id.starts_with("call_notice_")));
        assert!(after.transcript[notification - 1]
            .tool_calls
            .iter()
            .any(|call| Some(call.tool_call_id.as_str())
                == after.transcript[notification].tool_call_id.as_deref()));
        assert_eq!(
            after
                .transcript
                .iter()
                .filter(|m| m.tool_call_id.as_deref() == Some("slow-call"))
                .count(),
            1
        );
        assert!(world.events(&id).iter().any(|e| matches!(&e.event,
            SessionEvent::StepStarted { deliveries, .. } if deliveries.iter().any(|d| matches!(d,
                ToolDelivery::Result { invocation, mode: ToolDeliveryMode::Notification, .. } if invocation.provider_call_id == "slow-call"
            ))
        )));
        let before_restart = after.transcript.clone();
        after.respond_text("done").unwrap();
        world
            .wait_for_state(&id, |s| s.last_turn_outcome == Some(TurnOutcome::Finished))
            .await;
        world.restart().await.unwrap();
        world.send_mail(&id, "after restart").await.unwrap();
        let replay = world.request().await;
        assert_eq!(
            &replay.transcript[..before_restart.len()],
            before_restart.as_slice()
        );
        replay.respond_text("done").unwrap();
        world
            .wait_for_state(&id, |s| s.last_turn_outcome == Some(TurnOutcome::Finished))
            .await;
        world.shutdown().await;
    }
}
