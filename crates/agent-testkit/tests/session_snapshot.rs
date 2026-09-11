use serde_json::json;
use zork_agent::session::{
    events::{Selection, SessionEvent, TurnOutcome},
    recovery::recover_latest,
    state::{migrate_snapshot, snapshot_value, STATE_SCHEMA_VERSION},
    store::SessionStore,
};
use zork_agent_testkit::TestWorld;

#[test]
fn missing_snapshot_origin_never_falls_back_to_the_archive_for_an_overview() {
    use zork_agent::session::{
        query::{
            Commit, QueryError, ReadResult, ReadSummary, SessionDiscovery, SessionQuery,
            SessionReadHint, SnapshotWindow, WindowOrigin,
        },
        store::EventEnvelope,
        tools::ToolRegistry,
    };
    struct Unanchored;
    impl SessionQuery for Unanchored {
        fn exists(&self, _: &str) -> bool {
            true
        }
        fn discover_sessions(&self) -> Result<Vec<SessionDiscovery>, QueryError> {
            unreachable!()
        }
        fn last_commit(
            &self,
            _: &str,
            _: Option<&SessionReadHint>,
        ) -> Result<ReadResult<Option<Commit>>, QueryError> {
            unreachable!()
        }
        fn snapshot_windows(
            &self,
            _: &str,
            _: Option<&SessionReadHint>,
            visit: &mut dyn FnMut(SnapshotWindow) -> bool,
        ) -> Result<ReadSummary, QueryError> {
            assert!(visit(SnapshotWindow {
                origin: WindowOrigin::Unanchored,
                commits: vec![]
            }));
            Ok(ReadSummary::default())
        }
        fn all_commits_forward(
            &self,
            _: &str,
            _: &mut dyn FnMut(Commit) -> bool,
        ) -> Result<ReadSummary, QueryError> {
            panic!("aggregate read attempted archive replay")
        }
        fn scan_after(
            &self,
            _: &str,
            _: Option<&str>,
            _: &mut dyn FnMut(EventEnvelope) -> bool,
        ) -> Result<(), QueryError> {
            panic!("aggregate read attempted history scan")
        }
        fn before(
            &self,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Vec<EventEnvelope>, QueryError> {
            panic!("aggregate read attempted history paging")
        }
        fn event(&self, _: &str, _: &str) -> Result<Option<EventEnvelope>, QueryError> {
            unreachable!()
        }
    }
    assert!(
        recover_latest(&Unanchored, "session", None, &ToolRegistry::default())
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn aggregates_survive_snapshot_recovery_and_continue_from_the_append_suffix() {
    let mut world = TestWorld::new();
    let id = world
        .create_session(
            Selection {
                profile_id: "test-profile".into(),
                model: "test-model".into(),
                thinking: "medium".into(),
            },
            None,
            "/virtual/snapshot",
        )
        .await
        .unwrap();
    for turn in 0..3 {
        world.send_mail(&id, format!("turn {turn}")).await.unwrap();
        world
            .request()
            .await
            .respond_call(format!("end-{turn}"), "end", json!({}))
            .unwrap();
        world
            .wait_for_state(&id, |s| {
                s.active_turn.is_none() && s.last_turn_outcome == Some(TurnOutcome::Finished)
            })
            .await;
    }
    let before = world.state(&id).await.unwrap();
    assert!(before.overview.aggregates.complete);
    assert_eq!(before.overview.aggregates.run_count, 3);
    assert_eq!(before.overview.aggregates.usage.input, 3);
    assert_eq!(before.overview.aggregates.usage.reported_steps, 3);
    assert_eq!(before.overview.aggregates.recent.len(), 2);
    world.shutdown().await;
    let saved = world
        .store
        .append_snapshot(&id, STATE_SCHEMA_VERSION, snapshot_value(&before).unwrap())
        .unwrap();
    let recovered = recover_latest(&world.query, &id, None, &world.tools)
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.state.overview.aggregates,
        before.overview.aggregates
    );
    assert_eq!(
        recovered.state.overview.cursor.as_ref(),
        Some(&saved.envelope.event_id)
    );

    // Old snapshots have unknown coverage. Loading one must not quietly
    // substitute a count derived from whichever detail pages happen to exist.
    let mut legacy = snapshot_value(&before).unwrap();
    legacy.as_object_mut().unwrap().remove("overview");
    let legacy = migrate_snapshot(STATE_SCHEMA_VERSION, legacy, &world.tools).unwrap();
    assert!(!legacy.overview.aggregates.complete);

    let mut rotated = before.clone();
    rotated
        .apply(
            &serde_json::from_value(json!({
                "kind":"context_applied", "generation":before.generation.number+1,
                "purpose":"handoff", "document":"new context", "retained":{"start":0,"end":0},
                "tools":[], "carried_tools":[], "applied_at_ms":1,
            }))
            .unwrap(),
            &world.tools,
        )
        .unwrap();
    assert_eq!(rotated.overview.aggregates, before.overview.aggregates);
    assert!(rotated.overview().context_tokens.is_none());
    assert!(rotated.overview.last_provider.is_none());

    world.restart().await.unwrap();
    let scans = world.query.history_scan_calls(&id);
    let opening = world.service_handle().overview(&id).await.unwrap();
    assert_eq!(opening.aggregates, before.overview.aggregates);
    assert_eq!(world.query.history_scan_calls(&id), scans);
    world.send_mail(&id, "after restart").await.unwrap();
    world.request().await.respond_text("finished").unwrap();
    let after = world
        .wait_for_state(&id, |s| {
            s.active_turn.is_none() && s.overview.aggregates.run_count == 4
        })
        .await;
    assert_eq!(after.overview.aggregates.usage.input, 4);
    assert_eq!(after.overview.aggregates.usage.reported_steps, 4);
    assert_eq!(
        after.overview.aggregates.recent,
        before.overview.aggregates.recent
    );

    let mut folded = before.clone();
    let duplicate = world
        .events(&id)
        .into_iter()
        .find(|e| matches!(&e.event, SessionEvent::StepCompleted { .. }))
        .unwrap();
    let _ = folded.apply(&duplicate.event, &world.tools);
    assert_eq!(
        folded.overview.aggregates.usage, before.overview.aggregates.usage,
        "a repeated terminal event must not count twice"
    );
    world.shutdown().await;
}
