//! Transient browser resource ownership. Close bypasses the network command lock,
//! including when the view is dismissed while its first connection is still opening.
use crate::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
use zork_mesh::services::{LocalService, ServiceLink};

#[derive(Default)]
pub(crate) struct Views(Mutex<HashMap<String, (String, Option<LocalService>)>>);
impl Views {
    pub fn close(&self, id: &str) {
        self.0.lock().expect("service views").remove(id);
    }
    pub fn clear(&self) {
        self.0.lock().expect("service views").clear();
    }
    pub fn remove_peer(&self, origin: &str) {
        self.0
            .lock()
            .expect("service views")
            .retain(|_, (url, _)| ServiceLink::parse(url).is_ok_and(|link| link.origin != origin));
    }
}
impl Client {
    pub(crate) async fn open_service(&mut self, view_id: String, url: String) -> Result<Value> {
        ensure!(
            !view_id.is_empty() && view_id.len() <= 128,
            "invalid_service_view"
        );
        let link = ServiceLink::parse(&url)?;
        let peer = self
            .store
            .nodes()?
            .into_iter()
            .find(|node| {
                node.mesh
                    .as_ref()
                    .is_some_and(|remote| remote.origin == link.origin)
            })
            .context("服务节点尚未接入此客户端")?;
        ensure!(!self.store.replica_revoked(&peer.id)?, "设备访问权限已撤销");
        // Resume can clear old resources, so reserve only after it has completed.
        self.resume().await?;
        let node = self.node()?;
        {
            let mut views = self.services.0.lock().expect("service views");
            if let Some((original, service)) = views.get(&view_id) {
                ensure!(original == &url, "service_view_already_open");
                return Ok(json!({"url":service.as_ref().context("service_view_opening")?.url}));
            }
            ensure!(views.len() < 16, "too_many_service_views");
            views.insert(view_id.clone(), (url, None));
        }
        let opened = LocalService::open(node, &link).await;
        let mut views = self.services.0.lock().expect("service views");
        match opened {
            Ok(service) => {
                let slot = views.get_mut(&view_id).context("service_view_closed")?;
                let value = json!({"url":service.url});
                slot.1 = Some(service);
                Ok(value)
            }
            Err(error) => {
                views.remove(&view_id);
                Err(error)
            }
        }
    }
}
