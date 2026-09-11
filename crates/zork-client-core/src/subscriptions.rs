//! Serialized observation over the same controllers as native UI. Waiting is
//! separate from preparing a batch, and neither requires the command executor.
mod conversation;
mod settings;
mod snapshot;
#[cfg(test)]
mod tests;
use crate::{state::Device, store::ClientStore};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use zork_observe::Readiness;

#[derive(Deserialize)]
#[serde(tag = "projection", rename_all = "snake_case", deny_unknown_fields)]
pub enum Key {
    Invitation,
    Conversation {
        peer: String,
        session: Option<String>,
    },
    Settings {
        peer: String,
    },
}
impl Key {
    pub fn peer(&self) -> &str {
        match self {
            Self::Invitation => "",
            Self::Conversation { peer, .. } | Self::Settings { peer } => peer,
        }
    }
}

/// One waiter for each underlying source. Cancellation does not consume data.
pub struct Signals(Vec<Readiness>);
impl Signals {
    pub async fn changed(&mut self) -> Result<bool, zork_observe::Closed> {
        futures_util::future::poll_fn(|cx| {
            use std::task::Poll;
            let mut changed = false;
            let mut closed = false;
            for signal in &mut self.0 {
                match signal.poll_changed(cx) {
                    Poll::Ready(Ok(())) => changed = true,
                    Poll::Ready(Err(_)) => closed = true,
                    Poll::Pending => {}
                }
            }
            if changed {
                Poll::Ready(Ok(self
                    .0
                    .iter_mut()
                    .fold(false, |urgent, s| s.take_urgent() || urgent)))
            } else if closed {
                Poll::Ready(Err(zork_observe::Closed))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

enum Projection {
    Invitation(snapshot::SnapshotWire),
    Conversation(conversation::ConversationWire),
    Settings(settings::SettingsWire),
}
pub struct WireSubscription {
    projection: Projection,
    device: Option<Arc<Device>>,
    prepared: Option<(u64, Arc<Value>, bool)>,
    sequence: u64,
    applied: u64,
}
impl WireSubscription {
    pub(crate) fn from_invitation(source: &zork_observe::ValueSource<Value>) -> Self {
        Self {
            projection: Projection::Invitation(snapshot::SnapshotWire::new(source)),
            device: None,
            prepared: None,
            sequence: 0,
            applied: 0,
        }
    }
    /// Adapt an already-owned controller. Its lifecycle is independent of this
    /// read-only observer; the caller activates business operations separately.
    pub fn from_device(key: Key, device: Arc<Device>, store: Arc<ClientStore>) -> Result<Self> {
        anyhow::ensure!(
            device.bound_peer().is_none_or(|peer| peer == key.peer()),
            "projection belongs to another device"
        );
        let projection = match key {
            Key::Invitation => anyhow::bail!("invitation uses the client invitation source"),
            Key::Conversation { peer, session } => Projection::Conversation(
                conversation::ConversationWire::new(peer, session, device.clone(), store)?,
            ),
            Key::Settings { peer } => {
                Projection::Settings(settings::SettingsWire::new(peer, device.clone(), store)?)
            }
        };
        Ok(Self {
            projection,
            device: Some(device),
            prepared: None,
            sequence: 0,
            applied: 0,
        })
    }
    pub fn signals(&self) -> Signals {
        Signals(match &self.projection {
            Projection::Invitation(p) => p.signals(),
            Projection::Conversation(p) => p.signals(),
            Projection::Settings(p) => p.signals(),
        })
    }
    pub fn valid(&self, batch: u64) -> bool {
        self.prepared.as_ref().is_some_and(|(id, _, revoked)| {
            *id == batch
                && (*revoked
                    || self
                        .device
                        .as_ref()
                        .is_none_or(|device| !device.snapshot().revoked))
        }) && match &self.projection {
            Projection::Invitation(p) => p.valid(),
            Projection::Conversation(p) => p.valid(),
            Projection::Settings(p) => p.valid(),
        }
    }
    pub fn prepare(&mut self) -> Result<Option<Arc<Value>>> {
        if let Some((batch, _, _)) = &self.prepared {
            if !self.valid(*batch) {
                self.finish(*batch, false);
            }
        }
        if let Some((_, value, _)) = &self.prepared {
            return Ok(Some(value.clone()));
        }
        let value = match &mut self.projection {
            Projection::Invitation(p) => p.prepare()?,
            Projection::Conversation(p) => p.prepare()?,
            Projection::Settings(p) => p.prepare()?,
        };
        let Some((mut value, reset, revoked)) = value else {
            return Ok(None);
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("wire sequence exhausted");
        value["batch"] = json!(self.sequence);
        value["from"] = json!(self.applied);
        value["reset"] = json!(reset || self.applied == 0);
        let value = Arc::new(value);
        self.prepared = Some((self.sequence, value.clone(), revoked));
        Ok(Some(value))
    }
    pub fn finish(&mut self, batch: u64, applied: bool) -> bool {
        if self.prepared.as_ref().is_none_or(|(id, _, _)| *id != batch) {
            return false;
        }
        let valid = self.valid(batch);
        let accepted = match &mut self.projection {
            Projection::Invitation(p) => p.finish(applied && valid),
            Projection::Conversation(p) => p.finish(applied && valid),
            Projection::Settings(p) => p.finish(applied && valid),
        };
        if applied && valid && accepted {
            self.applied = batch;
        }
        self.prepared = None;
        valid && accepted
    }
    /// Explicitly enlarge this observer's history range; other observers keep
    /// their own bounded window. Loading remains a core business operation.
    pub fn older(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.prepared.is_none(),
            "finish the prepared frame before changing its range"
        );
        if let Projection::Conversation(p) = &mut self.projection {
            p.older();
        }
        Ok(())
    }
    pub fn newer(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.prepared.is_none(),
            "finish the prepared frame before changing its range"
        );
        if let Projection::Conversation(p) = &mut self.projection {
            p.newer();
        }
        Ok(())
    }
    /// Pin the requested range to a business record while reading history;
    /// None selects the latest tail. Scroll and frame clocks stay in the UI.
    pub fn window_anchor(&mut self, anchor: Option<String>) -> Result<()> {
        anyhow::ensure!(
            self.prepared.is_none(),
            "finish the prepared frame before changing its range"
        );
        if let Projection::Conversation(p) = &mut self.projection {
            p.window_anchor(anchor)?;
        }
        Ok(())
    }
}
