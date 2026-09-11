use super::*;
use zork_client_types::chat::{PreferenceChanges, UpdatePreferences};
use zork_client_types::interaction::{AgentConfig, Selection};

fn database() -> (tempfile::TempDir, GatewayDb, Channel) {
    let root = tempfile::tempdir().unwrap();
    let db = GatewayDb::open(root.path(), &root.path().join("workspaces")).unwrap();
    let receipt = db.chat_begin("chat", "chat").unwrap();
    let chat = db.create_chat("chat", &receipt.object_id, "Work").unwrap();
    (root, db, chat)
}

fn config() -> AgentConfig {
    AgentConfig {
        name: "Builder".into(),
        selection: Selection {
            profile_id: "profile".into(),
            model: "model".into(),
            thinking: "off".into(),
        },
        avatar: None,
        instructions: "Implement the requested changes".into(),
        skill_paths: vec![],
        allowed_leaders: vec!["leader".into()],
    }
}

fn worker(id: &str) -> NodeAgent {
    let config = config();
    NodeAgent {
        id: id.into(),
        name: config.name,
        role: AgentRole::Worker,
        avatar: None,
        profile_id: config.selection.profile_id,
        model: config.selection.model,
        thinking: config.selection.thinking,
        instructions: config.instructions,
        skill_paths: vec![],
        allowed_leaders: config.allowed_leaders,
        session_key: None,
        session_id: None,
    }
}

fn propose(db: &GatewayDb, chat: &Channel, request: Request) -> Message {
    let key = ulid::Ulid::new().to_string();
    let receipt = db.chat_begin(&key, &key).unwrap();
    db.post_chat_content(
        &key,
        &receipt.object_id,
        &chat.chat_id,
        &Author {
            id: "leader".into(),
            kind: AuthorKind::Agent,
            name: Some("Leader".into()),
        },
        "Please confirm",
        &[],
        None,
        &[],
        &[],
        Some(&MessageContent::request(request)),
    )
    .unwrap()
}

fn accept(id: &str) -> Response {
    Response {
        response_id: id.into(),
        accept: true,
        values: Default::default(),
    }
}

#[test]
fn competing_confirmations_commit_one_worker_and_one_result_and_recover_after_restart() {
    let (root, db, chat) = database();
    let db = std::sync::Arc::new(db);
    db.chat_begin("subscribe", "subscribe").unwrap();
    db.update_chat_preferences(
        "subscribe",
        &chat.chat_id,
        "leader",
        &UpdatePreferences {
            changes: PreferenceChanges {
                subscribed: Some(true),
                ..Default::default()
            },
            expected_revision: None,
            start: None,
        },
    )
    .unwrap();
    let original = propose(&db, &chat, Request::CreateAgent { config: config() });
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let db = db.clone();
            let chat = chat.chat_id.clone();
            let source = original.message_id.clone();
            std::thread::spawn(move || {
                db.respond_to_interaction(
                    &chat,
                    &source,
                    &accept(&format!("response-{i}")),
                    Some(&worker(&format!("worker-{i}"))),
                    "user",
                )
                .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(db.node_agents().unwrap().len(), 1);
    assert_eq!(db.chat(&chat.chat_id).unwrap().channel.message_count, 2);
    assert_eq!(
        db.chat_message(&chat.chat_id, &original.message_id)
            .unwrap(),
        original
    );
    assert_eq!(results[0].author.kind, AuthorKind::System);
    assert_eq!(db.chat_participants(&chat.chat_id).unwrap().len(), 1);
    let notices = db.chat_notice_page("local", None, 0).unwrap();
    assert_eq!(notices.items.len(), 1);
    assert_eq!(notices.items[0].message, results[0]);
    drop(db);
    let db = GatewayDb::open(root.path(), &root.path().join("workspaces")).unwrap();
    let recovered = db
        .respond_to_interaction(
            &chat.chat_id,
            &original.message_id,
            &accept("after-restart"),
            Some(&worker("unused")),
            "user",
        )
        .unwrap();
    assert_eq!(recovered, results[0]);
    assert_eq!(db.node_agents().unwrap().len(), 1);
}

#[test]
fn result_commit_failure_rolls_back_the_worker_and_retry_uses_the_same_request() {
    let (_root, db, chat) = database();
    let original = propose(&db, &chat, Request::CreateAgent { config: config() });
    db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON chat_interaction_messages WHEN NEW.request_message_id IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected result failure'); END;").unwrap();
    assert!(db
        .respond_to_interaction(
            &chat.chat_id,
            &original.message_id,
            &accept("response"),
            Some(&worker("worker")),
            "user"
        )
        .is_err());
    assert!(db.node_agent("worker").unwrap().is_none());
    assert_eq!(db.chat(&chat.chat_id).unwrap().channel.message_count, 1);
    db.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_result")
        .unwrap();
    let result = db
        .respond_to_interaction(
            &chat.chat_id,
            &original.message_id,
            &accept("response"),
            Some(&worker("worker")),
            "user",
        )
        .unwrap();
    assert!(db.node_agent("worker").unwrap().is_some());
    assert_eq!(result.reply_to, Some(original.message_id));
}

#[test]
fn stale_updates_cannot_overwrite_configuration_and_can_still_be_declined() {
    let (_root, db, chat) = database();
    let worker = worker("worker");
    db.insert_node_agent(&worker).unwrap();
    let original = propose(
        &db,
        &chat,
        Request::UpdateAgent {
            agent_id: worker.id.clone(),
            expected_revision: configuration_revision(&worker).unwrap(),
            config: config(),
        },
    );
    db.update_agent_avatar("worker", "owl").unwrap();
    assert!(db
        .respond_to_interaction(
            &chat.chat_id,
            &original.message_id,
            &accept("stale"),
            Some(&worker),
            "user"
        )
        .is_err());
    assert_eq!(
        db.node_agent("worker").unwrap().unwrap().avatar.as_deref(),
        Some("owl")
    );
    assert!(db
        .interaction_result(&chat.chat_id, &original.message_id)
        .unwrap()
        .is_none());
    let result = db
        .respond_to_interaction(
            &chat.chat_id,
            &original.message_id,
            &Response {
                response_id: "decline".into(),
                accept: false,
                values: Default::default(),
            },
            None,
            "user",
        )
        .unwrap();
    assert!(matches!(
        MessageContent::parse(result.interaction.as_ref().unwrap())
            .unwrap()
            .content,
        Content::Result {
            result: Resolution {
                outcome: Outcome::Declined,
                ..
            }
        }
    ));
}
