//! Durable delivery shared by desktop and mobile. Commit attempted before IO;
//! remove only after an acknowledgement. Retries retain the original request ID.
use crate::{api::GatewayClient, store::ClientStore};
use serde::Serialize;
#[derive(Clone, Default, Serialize)]
pub struct DeliveryReport {
    pub delivered: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
pub async fn flush(client: &GatewayClient, store: &ClientStore, node: &str) -> DeliveryReport {
    let _serial = client.delivery_gate.lock().await;
    let mut report = DeliveryReport::default();
    let result: anyhow::Result<()> = async {
        for pending in store.outbox(node)? {
            let Some(message) = store.begin_delivery(node, &pending.request_id)? else {
                continue;
            };
            let mut guard = AttemptGuard {
                store,
                node,
                id: &message.request_id,
                settled: false,
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(
                    if zork_client_types::files::decode(&message.content).is_some() {
                        120
                    } else {
                        15
                    },
                ),
                deliver(client, store, node, &message),
            )
            .await;
            guard.settled = true;
            match result {
                Ok(Ok(_)) => store.acknowledge(node, &message.request_id)?,
                result => {
                    if !store
                        .outbox(node)?
                        .iter()
                        .any(|m| m.request_id == message.request_id)
                    {
                        report.delivered.push(message.request_id.clone());
                        continue;
                    }
                    let error = match result {
                        Ok(Err(e)) => e.to_string(),
                        _ => "发送超时".into(),
                    };
                    store.fail_delivery(node, &message.request_id, &error)?;
                    report.error = Some(error);
                    continue;
                }
            }
            report.delivered.push(message.request_id.clone());
        }
        for pending in store.interaction_deliveries(node)? {
            if pending.submission.attempted || pending.submission.error.is_some() {
                continue;
            }
            if !store.change_interaction(node, &pending, |s| s.attempted = true)? {
                continue;
            }
            let mut guard = InteractionGuard {
                store,
                node,
                pending: pending.clone(),
                settled: false,
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                deliver_interaction(client, store, node, &pending),
            )
            .await;
            guard.settled = true;
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error.to_string()),
                Err(_) => Some("Submission timed out; recover the original response.".into()),
            };
            if let Some(error) = error {
                store.change_interaction(node, &pending, |s| s.error = Some(error.clone()))?;
                report.error = Some(error);
            }
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        report.error = Some(error.to_string());
    }
    report
}

async fn deliver_interaction(
    client: &GatewayClient,
    store: &ClientStore,
    node: &str,
    pending: &crate::store::InteractionDelivery,
) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    if !pending.submission.accepted {
        let value = client
            .node_request(
                reqwest::Method::POST,
                format!(
                    "/v1/node/chats/{}/messages/{}/respond",
                    pending.session, pending.message_id
                ),
                Some(serde_json::to_value(&pending.submission.response)?),
            )
            .await?;
        let message: crate::api::TranscriptMessage =
            serde_json::from_value(value["message"].clone())?;
        let crate::api::TranscriptMessage::Message { metadata, .. } = &message;
        let result = crate::interactions::result(metadata)
            .context("Missing authoritative interaction result")?;
        ensure!(
            result.request_message_id == pending.message_id,
            "Interaction response belongs to another request"
        );
        store.change_interaction(node, pending, |s| s.accepted = true)?;
    }
    // A command receipt can arrive ahead of intermediate Chat messages. Catch
    // up from the durable source tail instead of inserting that receipt as a
    // fictitious contiguous page or advancing the stream past unseen messages.
    let tail = store.source_message_tail(node, &pending.session, pending.generation)?;
    let page = client
        .catch_up_messages(&pending.session, tail.as_deref(), 100)
        .await?;
    store.cache_message_page_at(node, &pending.session, &page, None, pending.generation)?;
    ensure!(
        !store
            .interaction_submissions(node, &pending.session, pending.generation)?
            .contains_key(&pending.message_id),
        "Interaction result has not reached the message stream yet"
    );
    Ok(())
}

async fn deliver(
    client: &GatewayClient,
    store: &ClientStore,
    node: &str,
    message: &crate::store::QueuedMessage,
) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use zork_client_types::files;
    if let Some((_, references)) = files::decode(&message.content) {
        ensure!(files::valid(&references), "invalid attachments");
        for file in references {
            let bytes = store
                .blob(node, &format!("upload:{}", file.id))?
                .context("attachment snapshot missing")?;
            ensure!(
                bytes.len() == file.byte_len
                    && zork_mesh::content_root(&bytes) == file.content_root,
                "attachment snapshot changed"
            );
            let mut offset = 0;
            loop {
                let end = (offset + files::CHUNK_BYTES).min(bytes.len());
                let reply = client.node_request(reqwest::Method::POST,
                    format!("/v1/im/sessions/{}/files", message.session_id),
                    Some(serde_json::json!({"file":file,"offset":offset,"bytes":&bytes[offset..end]}))).await?;
                let received = reply["received"]
                    .as_u64()
                    .context("invalid upload receipt")? as usize;
                ensure!(
                    received >= end && received <= bytes.len(),
                    "invalid upload offset"
                );
                if received == bytes.len() {
                    break;
                }
                ensure!(received > offset, "upload made no progress");
                offset = received;
            }
        }
    }
    client
        .post_message_id(&message.session_id, &message.content, &message.request_id)
        .await?;
    Ok(())
}

/// Dispatch each explicit send once. Reconnection never retries a failed request.
/// The store marks interrupted requests failed when reopened.
pub struct DeliveryPump {
    online: tokio::sync::watch::Sender<bool>,
    reports: tokio::sync::watch::Receiver<DeliveryReport>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for DeliveryPump {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl DeliveryPump {
    pub fn start(
        client: std::sync::Arc<GatewayClient>,
        store: std::sync::Arc<ClientStore>,
        node: String,
    ) -> Self {
        let (online, _connection) = tokio::sync::watch::channel(false);
        let (reports_tx, reports) = tokio::sync::watch::channel(DeliveryReport::default());
        let executor = client.clone();
        let mut changes = store.delivery_events();
        let task = executor.spawn(async move {
            loop {
                changes.borrow_and_update();
                let messages = store.outbox(&node).is_ok_and(|messages| {
                    messages.iter().any(|m| !m.attempted && m.error.is_none())
                });
                let interactions = store.interaction_deliveries(&node).is_ok_and(|items| {
                    items
                        .iter()
                        .any(|i| !i.submission.attempted && i.submission.error.is_none())
                });
                if messages || interactions {
                    reports_tx.send_replace(flush(&client, &store, &node).await);
                } else if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        Self {
            online,
            reports,
            task,
        }
    }
    pub fn set_connected(&self, connected: bool) {
        self.online.send_if_modified(|old| {
            if *old == connected {
                false
            } else {
                *old = connected;
                true
            }
        });
    }
    pub fn reports(&self) -> tokio::sync::watch::Receiver<DeliveryReport> {
        self.reports.clone()
    }
    pub fn take_report(&mut self) -> Option<DeliveryReport> {
        self.reports
            .has_changed()
            .ok()
            .filter(|changed| *changed)
            .map(|_| self.reports.borrow_and_update().clone())
    }
}

struct AttemptGuard<'a> {
    store: &'a ClientStore,
    node: &'a str,
    id: &'a str,
    settled: bool,
}
impl Drop for AttemptGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self
                .store
                .fail_delivery(self.node, self.id, "发送中断，请手动重发");
        }
    }
}

struct InteractionGuard<'a> {
    store: &'a ClientStore,
    node: &'a str,
    pending: crate::store::InteractionDelivery,
    settled: bool,
}
impl Drop for InteractionGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self
                .store
                .change_interaction(self.node, &self.pending, |s| {
                    s.error = Some("Submission interrupted; recover the original response.".into())
                });
        }
    }
}
