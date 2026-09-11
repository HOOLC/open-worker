use std::{
    fs,
    sync::{Arc, RwLock},
};
use zork_agent::session::{service::ServiceOptions, wire::SessionSelection};
use zork_agent::skills::CATALOG_NOTICE;
use zork_agent_testkit::TestWorld;

#[tokio::test(flavor = "multi_thread")]
async fn skill_catalog_is_durable_refreshes_without_rewriting_prefix_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("build")).unwrap();
    fs::write(
        root.path().join("build/SKILL.md"),
        "---\nname: build\ndescription: Build on this device\n---\nSecret full instructions",
    )
    .unwrap();
    let paths = Arc::new(RwLock::new(vec![root.path().to_owned()]));
    let mut options = ServiceOptions::default();
    options.runner.skill_sources = Some({
        let paths = paths.clone();
        Arc::new(move |_| Ok(paths.read().unwrap().clone()))
    });
    let mut world = TestWorld::with_options(options);
    let id = world
        .create_session(
            SessionSelection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            root.path().to_string_lossy(),
        )
        .await
        .unwrap();
    world.send_mail(&id, "start").await.unwrap();
    let first = world.request().await;
    let original = first.transcript.clone();
    assert_eq!(
        original
            .iter()
            .filter(|m| m.content.contains(CATALOG_NOTICE))
            .count(),
        1
    );
    assert!(original
        .iter()
        .any(|m| m.content.contains("Build on this device")));
    assert!(!original
        .iter()
        .any(|m| m.content.contains("Secret full instructions")));
    world.send_mail(&id, "continue").await.unwrap();
    first.respond_text("working").unwrap();
    let second = world.request().await;
    assert_eq!(
        second
            .transcript
            .iter()
            .filter(|m| m.content.contains(CATALOG_NOTICE))
            .count(),
        1
    );
    assert_eq!(&second.transcript[..original.len()], original.as_slice());
    paths.write().unwrap().clear();
    world.send_mail(&id, "sources changed").await.unwrap();
    second.respond_text("working").unwrap();
    let third = world.request().await;
    let current = third
        .transcript
        .iter()
        .rev()
        .find(|m| m.content.contains(CATALOG_NOTICE))
        .unwrap();
    assert!(!current.content.contains("Build on this device"));
    assert!(current.content.contains("\"skills\":[]"));
    assert_eq!(&third.transcript[..original.len()], original.as_slice());
    third.respond_text("done").unwrap();
    world
        .wait_for_state(&id, |state| state.active_turn.is_none())
        .await;
    world.restart().await.unwrap();
    world.send_mail(&id, "after restart").await.unwrap();
    let restarted = world.request().await;
    let current = restarted
        .transcript
        .iter()
        .rev()
        .find(|m| m.content.contains(CATALOG_NOTICE))
        .unwrap();
    assert!(!current.content.contains("Build on this device"));
    assert!(current.content.contains("\"skills\":[]"));
    restarted.respond_text("done").unwrap();
    world.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_successor_receives_current_skills_without_rediscovery_during_handoff() {
    use zork_agent::session::model::{ModelOutcome, ModelTokenUsage};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("SKILL.md");
    fs::write(
        &path,
        "---\nname: build\ndescription: Original description\n---\nbody",
    )
    .unwrap();
    let source = root.path().to_owned();
    let mut options = ServiceOptions::default();
    options.runner.context.strategy = zork_agent_api::ContextStrategy::Handoff;
    options.runner.input_budget = Arc::new(|_| Some(100));
    options.runner.skill_sources = Some(Arc::new(move |_| Ok(vec![source.clone()])));
    let mut world = TestWorld::with_options(options);
    let id = world
        .create_session(
            SessionSelection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            root.path().to_string_lossy(),
        )
        .await
        .unwrap();
    world.send_mail(&id, "start").await.unwrap();
    let first = world.request().await;
    world
        .send_mail(&id, "continue the same task")
        .await
        .unwrap();
    first
        .respond(Ok(ModelOutcome {
            text: "progress".into(),
            tool_calls: vec![],
            provider_context: None,
            provider_input: None,
            usage: Some(ModelTokenUsage {
                input_tokens: 100,
                cached_input_tokens: None,
                output_tokens: 1,
                output_reasoning_tokens: None,
                output_text_tokens: None,
            }),
        }))
        .unwrap();
    let handoff = world.request().await;
    assert!(handoff
        .transcript
        .iter()
        .any(|m| m.content.contains("A context handoff is required")));
    fs::write(
        &path,
        "---\nname: build\ndescription: Updated description\n---\nbody",
    )
    .unwrap();
    handoff
        .respond_call(
            "handoff",
            "handoff",
            serde_json::json!({"document":"Continue the task; work remains."}),
        )
        .unwrap();
    let successor = world.request().await;
    assert_eq!(successor.generation, 2);
    assert!(successor
        .transcript
        .iter()
        .any(|m| m.content.contains(CATALOG_NOTICE) && m.content.contains("Updated description")));
    successor.respond_text("done").unwrap();
    world.shutdown().await;
}
