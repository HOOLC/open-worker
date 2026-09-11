use serde_json::json;
use zork_client_core::{Client, Command};

#[test]
fn platform_preferences_are_local_and_persist_without_a_connected_peer() {
    let root = tempfile::tempdir().unwrap();
    let execute = |request| {
        let command: Command = serde_json::from_value(request).unwrap();
        assert!(command.is_local());
        Client::open(root.path()).unwrap().local().execute(command)
    };
    assert_eq!(
        execute(json!({"op":"preferences"})).unwrap()["message_preview_height"],
        0
    );
    assert_eq!(
        execute(json!({"op":"preferences","message_preview_height":480})).unwrap()
            ["message_preview_height"],
        480
    );
    assert!(execute(json!({"op":"preferences","message_preview_height":999})).is_err());
    assert_eq!(
        execute(json!({"op":"preferences"})).unwrap()["message_preview_height"],
        480
    );
}
