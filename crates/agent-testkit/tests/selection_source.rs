use std::sync::{Arc, Mutex};

use serde_json::json;
use zork_agent::session::{events::SessionEvent, service::ServiceOptions, wire::SessionSelection};
use zork_agent_testkit::TestWorld;

fn selection(model: &str) -> SessionSelection {
    SessionSelection {
        profile_id: "test-profile".into(),
        model: model.into(),
        thinking: "high".into(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn next_step_and_recovery_read_owner_selection_before_budgets() {
    let latest = Arc::new(Mutex::new(selection("first")));
    let mut options = ServiceOptions::default();
    options.runner.selection_source = Some({
        let latest = latest.clone();
        Arc::new(move |_| Ok(Some(latest.lock().unwrap().clone())))
    });
    options.runner.max_output_tokens = Arc::new(|state| {
        Some(if state.selection.as_ref().unwrap().model == "first" {
            1000
        } else {
            2000
        })
    });
    options.runner.input_budget = Arc::new(|state| {
        Some(if state.selection.as_ref().unwrap().model == "first" {
            10000
        } else {
            20000
        })
    });
    let mut world = TestWorld::with_options(options);
    let id = world
        .create_session(selection("stale"), None, "/virtual/selection")
        .await
        .unwrap();
    world.send_mail(&id, "start").await.unwrap();
    let first = world.request().await;
    assert_eq!(first.selection.model, "first");
    assert_eq!(first.max_output_tokens, Some(1000));

    *latest.lock().unwrap() = selection("second");
    // Updating the owner must not cancel or rewrite this in-flight request.
    assert_eq!(first.selection.model, "first");
    first
        .respond_call("call", "unknown.tool", json!({}))
        .unwrap();
    let second = world.request().await;
    assert_eq!(second.selection.model, "second");
    assert_eq!(second.max_output_tokens, Some(2000));
    assert!(world.events(&id).iter().any(|event| matches!(
        &event.event,
        SessionEvent::StepStarted { step_id, input_budget: Some(20000), .. }
            if step_id == &second.step_id
    )));
    assert!(world.events(&id).iter().any(|event| matches!(
        &event.event, SessionEvent::SelectionChanged { selection } if selection.model == "second"
    )));

    *latest.lock().unwrap() = selection("after-restart");
    world.restart().await.unwrap();
    drop(second);
    let recovered = world.request().await;
    assert_eq!(recovered.selection.model, "after-restart");
    assert_eq!(recovered.max_output_tokens, Some(2000));
    drop(recovered);
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unowned_session_keeps_its_explicit_selection() {
    let mut options = ServiceOptions::default();
    options.runner.selection_source = Some(Arc::new(|_| Ok(None)));
    let mut world = TestWorld::with_options(options);
    let id = world
        .create_session(selection("standalone"), None, "/virtual/standalone")
        .await
        .unwrap();
    world.send_mail(&id, "start").await.unwrap();
    let request = world.request().await;
    assert_eq!(request.selection.model, "standalone");
    drop(request);
    world.shutdown().await;
}
