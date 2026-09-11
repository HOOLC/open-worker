use super::*;
fn who() -> Subject {
    Subject {
        origin: "key:caller".into(),
        agent: "leader".into(),
        session: "session".into(),
    }
}
#[test]
fn node_receipts_survive_restart_without_reexecuting_commands() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::Store::open(dir.path()).unwrap();
    let rpc = Rpc {
        interrupt: false,
        subject: who(),
        invocation_id: "call".into(),
        tool: "device.exec".into(),
        arguments: json!({"command":"touch effect"}),
    };
    let (id, fresh) = store.accept(&rpc, "hash").unwrap();
    assert!(fresh);
    valid_id(&id).unwrap();
    store
        .finish(
            &id,
            "running",
            Some(json!({"pid":123,"cwd":"/workspace","process_state":"running"})),
        )
        .unwrap();
    drop(store);
    let store = store::Store::open(dir.path()).unwrap();
    let recovered = store.view(&id, &who()).unwrap();
    assert_eq!(recovered["state"], "outcome_unknown");
    assert_eq!(recovered["result"]["process_state"], "unknown");
    assert_eq!(recovered["result"]["pid"], 123);
    assert_eq!(recovered["result"]["cwd"], "/workspace");
    assert_eq!(store.accept(&rpc, "hash").unwrap(), (id.clone(), false));
    assert!(store.accept(&rpc, "changed").is_err());
    let mut stranger = who();
    stranger.session = "other".into();
    assert!(store.view(&id, &stranger).is_err());
}
