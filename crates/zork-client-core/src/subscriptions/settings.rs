use crate::{
    state::{Device, Domains},
    store::ClientStore,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use zork_observe::{BatchId, Readiness, ValueSource, ValueSubscription};

pub(super) struct SettingsWire {
    source: Arc<ValueSource<Value>>,
    updates: ValueSubscription<Value>,
    pending: Option<BatchId>,
    task: tokio::task::JoinHandle<()>,
    device: Arc<Device>,
}
impl Drop for SettingsWire {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl SettingsWire {
    pub(super) fn new(peer: String, device: Arc<Device>, store: Arc<ClientStore>) -> Result<Self> {
        // Register all hints before the first committed projection read.
        let mut device_updates =
            device.subscribe_domains(Domains::CONNECTION | Domains::PROFILES | Domains::AGENTS);
        let mut profiles = device.profiles().subscribe_state();
        let mut stored = store.settings_events(&peer);
        device_updates.snapshot();
        profiles.snapshot();
        stored.snapshot();
        let initial = crate::settings::snapshot(&store, &peer)?;
        let source = Arc::new(ValueSource::new(initial));
        let updates = source.subscribe();
        let publisher = source.clone();
        let owner = device.clone();
        let task = tokio::spawn(async move {
            loop {
                let now = crate::store::delivery_now_ms();
                let state = owner.snapshot();
                let expiration = state
                    .confirmed_at_ms
                    .map(|at| at.saturating_add(60_000))
                    .filter(|_| state.online == Some(true) && publisher.read()["online"] == true);
                tokio::select! {
                    update = device_updates.changed() => if update.is_none() { break; },
                    update = profiles.changed() => if update.is_none() { break; },
                    update = stored.changed() => if update.is_none() { break; },
                    _ = async { if let Some(at) = expiration { tokio::time::sleep(Duration::from_millis(at.saturating_sub(now))).await; }
                        else { std::future::pending::<()>().await; } } => {},
                }
                device_updates.snapshot();
                profiles.snapshot();
                stored.snapshot();
                let snapshot = match crate::settings::snapshot(&store, &peer) {
                    Ok(result) => result,
                    Err(error) => json!({"ready":false,"error":error.to_string()}),
                };
                if snapshot["revoked"] == true {
                    publisher.invalidate(snapshot);
                    break;
                }
                publisher.publish(snapshot);
            }
        });
        Ok(Self {
            source,
            updates,
            pending: None,
            task,
            device,
        })
    }
    pub(super) fn signals(&self) -> Vec<Readiness> {
        vec![self.updates.readiness()]
    }
    pub(super) fn valid(&self) -> bool {
        self.pending.is_some_and(|id| self.updates.valid(id))
    }
    pub(super) fn prepare(&mut self) -> Result<Option<(Value, bool, bool)>> {
        if self.device.snapshot().revoked && self.source.read()["revoked"] != true {
            self.source
                .invalidate(json!({"ready":false,"revoked":true,"error":"设备访问权限已撤销"}));
        }
        let Some(batch) = self.updates.prepare() else {
            return Ok(None);
        };
        self.pending = Some(batch.id);
        Ok(Some((
            json!({"changed":true,"snapshot":batch.snapshot.value}),
            batch.is_reset(),
            batch.snapshot.value["revoked"] == true,
        )))
    }
    pub(super) fn finish(&mut self, applied: bool) -> bool {
        self.pending.take().is_some_and(|batch| {
            if applied {
                self.updates.acknowledge(batch)
            } else {
                self.updates.discard(batch)
            }
        })
    }
}
