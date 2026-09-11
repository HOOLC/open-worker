use super::{Device, Subscription};
use crate::{delivery::DeliveryPump, store::QueuedMessage};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Default, PartialEq)]
pub struct Outbox {
    pub items: Arc<Vec<QueuedMessage>>,
    pub by_message_id: Arc<HashMap<String, usize>>,
    pub phases: Arc<HashMap<String, String>>,
}
impl Device {
    pub fn recover_outbox(&self) -> Arc<Outbox> {
        self.reload_outbox();
        self.outbox()
    }
    pub fn outbox(&self) -> Arc<Outbox> {
        self.outbox.read()
    }
    pub fn subscribe_outbox(&self) -> Subscription<Outbox> {
        self.outbox.subscribe()
    }
    pub fn start_delivery(self: &Arc<Self>) {
        let Some((store, node)) = &self.cache else {
            return;
        };
        let mut task = self.delivery_task.lock().unwrap();
        if task.is_some() {
            return;
        }
        self.reload_outbox();
        let mut changes = store.delivery_events();
        let pump = DeliveryPump::start(self.client.clone(), store.clone(), node.clone());
        let weak = Arc::downgrade(self);
        *task=Some(self.client.spawn(async move {
            let _pump=pump;
            loop {
                changes.borrow_and_update();
                let Some(device)=weak.upgrade() else {return;};
                device.reload_outbox();
                for conversation in device.conversations.lock().unwrap().values().filter_map(std::sync::Weak::upgrade).collect::<Vec<_>>() {
                    conversation.sync_interactions();
                }
                let now=crate::store::delivery_now_ms();
                let delay=device.outbox().items.iter().filter(|m|m.error.is_none() && m.sent_at_ms>0)
                    .map(|m|m.sent_at_ms.saturating_add(1000)).filter(|deadline|*deadline>now).min().map(|deadline|std::time::Duration::from_millis(deadline-now));
                drop(device);
                if let Some(delay)=delay {
                    tokio::select! { _=tokio::time::sleep(delay)=>{}, result=changes.changed()=>if result.is_err(){return;} }
                } else if changes.changed().await.is_err() {return;}
            }
        }));
    }
    pub(super) fn reload_outbox(&self) {
        let serial = self.outbox_gate.lock().unwrap();
        let items = self
            .cache
            .as_ref()
            .and_then(|(store, node)| store.outbox(node).ok())
            .unwrap_or_default();
        let by_message_id = items
            .iter()
            .enumerate()
            .map(|(i, m)| (format!("client-{}-{}", m.session_id, m.request_id), i))
            .collect();
        let phases = items
            .iter()
            .map(|m| (m.request_id.clone(), m.delivery_status().to_owned()))
            .collect();
        let changed = self.outbox.publish(Outbox {
            items: Arc::new(items),
            by_message_id: Arc::new(by_message_id),
            phases: Arc::new(phases),
        });
        drop(serial);
        if changed {
            for conversation in self
                .conversations
                .lock()
                .unwrap()
                .values()
                .filter_map(std::sync::Weak::upgrade)
                .collect::<Vec<_>>()
            {
                conversation.sync_outbox();
            }
        }
    }
    /// Commit the explicit send and source-draft clearing together. Core then
    /// publishes both projections; the UI never owns an optimistic outbox copy.
    pub fn enqueue(&self, session: &str, content: String) -> anyhow::Result<QueuedMessage> {
        let _serial = self.draft_gate.lock().unwrap();
        self.enqueue_locked(session, content)
    }
    /// Compose the canonical draft, including comments and attachments, inside
    /// the same serialized command that commits its delivery and clears it.
    pub fn submit_draft(&self, session: &str, text: &str) -> anyhow::Result<Option<QueuedMessage>> {
        crate::valid_session(session)?;
        let _serial = self.draft_gate.lock().unwrap();
        let draft = self.draft(session);
        let content = zork_client_types::files::compose(
            &crate::comments::compose_document(text, &draft.comments, &draft.attachments),
            &draft.files,
        );
        if content.is_empty() {
            return Ok(None);
        }
        self.enqueue_locked(session, content).map(Some)
    }
    fn enqueue_locked(&self, session: &str, content: String) -> anyhow::Result<QueuedMessage> {
        crate::valid_session(session)?;
        crate::valid_content(&content, false)?;
        if let Some(summary) = self
            .snapshot()
            .sessions
            .iter()
            .find(|s| s.session_id == session)
        {
            anyhow::ensure!(
                crate::conversation::can_send(summary),
                "This conversation cannot accept messages"
            );
        }
        let (store, node) = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("persistent delivery is unavailable"))?;
        let message = QueuedMessage {
            request_id: ulid::Ulid::new().to_string(),
            session_id: session.into(),
            content,
            attempted: false,
            sent_at_ms: crate::store::delivery_now_ms(),
            ..Default::default()
        };
        store.enqueue_and_clear_draft(node, &message)?;
        self.publish_cleared_draft(session);
        self.reload_outbox();
        Ok(message)
    }
    pub fn withdraw_delivery(&self, id: &str) -> anyhow::Result<Option<QueuedMessage>> {
        let (store, node) = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("persistent delivery is unavailable"))?;
        let _serial = self.draft_gate.lock().unwrap();
        let message = store.withdraw_to_draft(node, id)?;
        if let Some(message) = &message {
            self.reload_draft(&message.session_id)?;
            if let Some(conversation) = self
                .conversations
                .lock()
                .unwrap()
                .get(&message.session_id)
                .and_then(std::sync::Weak::upgrade)
            {
                conversation.remove_failed(id);
            }
        }
        self.reload_outbox();
        Ok(message)
    }
    pub fn retry_delivery(&self, id: &str) -> anyhow::Result<()> {
        let (store, node) = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("persistent delivery is unavailable"))?;
        store.retry_delivery(node, id)?;
        self.reload_outbox();
        Ok(())
    }
    pub fn delete_failed_delivery(&self, id: &str) -> anyhow::Result<()> {
        let (store, node) = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("persistent delivery is unavailable"))?;
        let session = store
            .outbox(node)?
            .iter()
            .find(|m| m.request_id == id)
            .map(|m| m.session_id.clone());
        store.delete_failed(node, id)?;
        if let Some(session) = session {
            if let Some(conversation) = self
                .conversations
                .lock()
                .unwrap()
                .get(&session)
                .and_then(std::sync::Weak::upgrade)
            {
                conversation.remove_failed(id);
            }
        }
        self.reload_outbox();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn submission_uses_canonical_comments_and_attachments_and_clears_atomically() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::store::ClientStore::open(root.path()).unwrap());
        let device = Device::open(
            Arc::new(crate::api::GatewayClient::new("http://127.0.0.1:9", None)),
            Some((store.clone(), "node".into())),
            true,
        );
        let comment = crate::comments::DraftComment {
            id: "comment".into(),
            source: crate::comments::CommentSource {
                session_id: "chat".into(),
                quote: "selected text".into(),
                ..Default::default()
            },
            comment: "explain".into(),
        };
        let attachment = crate::comments::TextAttachment {
            id: "attachment".into(),
            name: "notes.txt".into(),
            content: "keep this attachment".into(),
        };
        device
            .edit_document(
                "chat",
                super::super::Draft {
                    text: "old input".into(),
                    comments: vec![comment.clone()],
                    attachments: vec![attachment.clone()],
                    files: vec![],
                },
            )
            .unwrap();
        let queued = device
            .submit_draft("chat", "current input")
            .unwrap()
            .unwrap();
        let (text, comments, attachments) =
            crate::comments::decode_document(&queued.content).unwrap();
        assert_eq!(text, "current input");
        assert_eq!(comments, vec![comment]);
        assert_eq!(attachments, vec![attachment]);
        assert_eq!(*device.draft("chat"), super::super::Draft::default());
        assert_eq!(store.outbox("node").unwrap().len(), 1);
        assert!(device.submit_draft("chat", " ").unwrap().is_none());
        assert_eq!(store.outbox("node").unwrap().len(), 1);
    }
}
