//! The native client uses the same channel transaction as Agent tools.
use super::*;
use crate::db::VisibleMessageRow;
use zork_client_types::{
    chat::{Author, AuthorKind},
    files,
};

pub async fn post(
    state: &AppState,
    session: &crate::db::SessionRow,
    request: &str,
    content: &str,
    reply_to: Option<&str>,
    mentions: &[String],
) -> Result<VisibleMessageRow> {
    let channel = state.db.chat(&session.key)?;
    let key = format!(
        "client-{}",
        fingerprint(&(&channel.channel.chat_id, request))?
    );
    let signature = fingerprint(&(content, reply_to, mentions))?;
    // The outbox, live echo and history must name the same message so core can
    // replace the pending row and acknowledge delivery by identity.
    let message_id = format!(
        "client-{}-{request}",
        session.id.as_deref().context("chat_session_missing")?
    );
    let receipt = state.db.chat_begin_with_id(&key, &signature, &message_id)?;
    if receipt.result.is_none() {
        let (text, refs) = files::decode(content).unwrap_or((content.into(), vec![]));
        ensure!(files::valid(&refs), "invalid_attachments");
        let files = refs
            .iter()
            .map(|file| {
                state.db.copy_chat_file(
                    &channel.channel.chat_id,
                    &channel.channel.chat_id,
                    &file.id,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            files.iter().map(|f| &f.reference).eq(refs.iter()),
            "attachment_reference_mismatch"
        );
        state.db.post_chat_message(
            &key,
            &receipt.object_id,
            &channel.channel.chat_id,
            &Author {
                id: "local-user".into(),
                kind: AuthorKind::User,
                name: None,
            },
            &text,
            &files,
            reply_to,
            mentions,
        )?;
    }
    let message = state.db.chat_visible_message(&receipt.object_id)?;
    state.entries.publish_visible_message(&message);
    Ok(message)
}
