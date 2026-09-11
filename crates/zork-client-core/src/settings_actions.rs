//! Business intents shared by platform adapters. HTTP is an implementation detail.
use crate::state::upgrade_status;
use crate::{
    model_edit::{ConnectionInput, ModelInput},
    Client,
};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SettingsAction {
    NodeInfo,
    CheckUpdate,
    Upgrade {
        version: String,
    },
    RenameDevice {
        name: String,
    },
    RenameProfile {
        profile: String,
        name: String,
    },
    SaveModel {
        profile: String,
        input: ModelInput,
    },
    RemoveModel {
        profile: String,
        model: Value,
    },
    EnableModel {
        profile: String,
        model: String,
        enabled: bool,
    },
    DiscoverModels {
        profile: String,
    },
    RefreshQuota {
        profile: String,
    },
    SaveConnection {
        input: ConnectionInput,
    },
    StartAuthorization {
        profile: String,
        provider: String,
        billing: String,
    },
    CompleteAuthorization {
        callback: String,
    },
    CancelAuthorization,
    AgentGrants {
        id: String,
        allowed: Vec<String>,
        expected: Vec<String>,
    },
    SaveAgent {
        input: AgentInput,
    },
    OpenAgent {
        id: String,
    },
    PrepareAgent {
        id: String,
    },
    StopConversation {
        session: String,
    },
}

pub use crate::agent_edit::AgentInput;

pub(crate) fn recover_operations(store: &crate::store::ClientStore) -> Result<()> {
    for node in store.nodes()? {
        if let Some(mut operation) = store.get::<Value>(&node.id, "node-operation")? {
            if operation["running"] == true {
                // The remote operation may still run after this client exits.
                // A persisted monitor flag cannot represent a live local job.
                operation["running"] = json!(false);
                operation["completed"] = json!(false);
                operation["uncertain"] = json!(true);
                operation["message"] = json!("上次升级结果待确认，请刷新设备状态");
                store.put(&node.id, "node-operation", &operation)?;
            }
        }
    }
    Ok(())
}

impl Client {
    pub(crate) async fn settings_action(
        &mut self,
        peer: String,
        action: SettingsAction,
    ) -> Result<Value> {
        self.peer(&peer)?;
        let gateway = self.gateway(&peer)?;
        let device = crate::state::Device::open(
            gateway.clone(),
            Some((self.store.clone(), peer.clone())),
            true,
        );
        device.start();
        self.devices.insert(peer.clone(), device.clone());
        let profiles = device.profiles();
        match action {
            SettingsAction::NodeInfo => self.request(&peer, "GET", "/v1/node/info", None).await,
            SettingsAction::CheckUpdate => {
                self.request(&peer, "GET", "/v1/node/update", None).await
            }
            SettingsAction::RenameDevice { name } => {
                let name = zork_config::membership::validate_device_name(&name)?;
                self.request(&peer, "PUT", "/v1/node/name", Some(json!({"name":name})))
                    .await
            }
            SettingsAction::RenameProfile { profile, name } => {
                profiles.rename(profile, name).await?;
                Ok(json!({}))
            }
            SettingsAction::SaveModel { profile, input } => {
                profiles.save_model(&profile, input).await?;
                Ok(json!({}))
            }
            SettingsAction::RemoveModel { profile, model } => {
                profiles.remove_model(&profile, model).await?;
                Ok(json!({}))
            }
            SettingsAction::EnableModel {
                profile,
                model,
                enabled,
            } => {
                profiles.set_model_enabled(&profile, model, enabled).await?;
                Ok(json!({}))
            }
            SettingsAction::DiscoverModels { profile } => profiles.discover_models(&profile).await,
            SettingsAction::RefreshQuota { profile } => {
                profiles.refresh_quota(profile).await;
                Ok(json!({}))
            }
            SettingsAction::SaveConnection { input } => {
                profiles.refresh().await;
                profiles.save_connection(input).await?;
                Ok(json!({}))
            }
            SettingsAction::StartAuthorization {
                profile,
                provider,
                billing,
            } => {
                profiles
                    .start_authorization(profile, provider, billing)
                    .await?;
                Ok(json!({}))
            }
            SettingsAction::CompleteAuthorization { callback } => {
                profiles.continue_authorization(callback);
                Ok(json!({}))
            }
            SettingsAction::CancelAuthorization => {
                profiles.cancel_authorization().await?;
                Ok(json!({}))
            }
            SettingsAction::AgentGrants {
                id,
                allowed,
                expected,
            } => {
                crate::model_edit::valid_id(&id)?;
                device
                    .agents()
                    .update_agent_grants(&id, allowed, json!(expected))
                    .await?;
                Ok(json!({}))
            }
            SettingsAction::SaveAgent { input } => device.agents().save(input).await,
            SettingsAction::OpenAgent { id } => {
                crate::model_edit::valid_id(&id)?;
                device.open_agent(&id).await
            }
            SettingsAction::PrepareAgent { id } => {
                crate::model_edit::valid_id(&id)?;
                if device.snapshot().online == Some(true) {
                    device.open_agent(&id).await
                } else {
                    Ok(json!({"cached":true}))
                }
            }
            SettingsAction::StopConversation { session } => {
                crate::valid_session(&session)?;
                device.conversation(&session).stop();
                Ok(json!({}))
            }
            SettingsAction::Upgrade { version } => {
                let current: Option<Value> = self.store.get(&peer, "node-operation")?;
                anyhow::ensure!(
                    !current.is_some_and(|v| v["running"] == true),
                    "设备升级仍在进行"
                );
                let id = ulid::Ulid::new().to_string();
                self.store.put(
                    &peer,
                    "node-operation",
                    &json!({"id":id,"version":version,"running":true,"message":"正在提交升级"}),
                )?;
                let store = self.store.clone();
                let job_id = id.clone();
                gateway.spawn(async move {
                    let outcome = device.upgrade(&version, |state| {
                            let status = upgrade_status(&state.info, &version).unwrap_or(&Value::Null);
                            let message = status["message"].as_str().filter(|s| !s.is_empty()).unwrap_or("等待设备恢复连接…");
                            store.put(&peer,"node-operation",&json!({"id":job_id,"version":version,"running":true,"message":message}))?;
                            Ok(())
                    }).await;
                    let _ = store.put(&peer,"node-operation",&json!({"id":job_id,"version":version,"running":false,"completed":outcome.is_ok(),"error":outcome.err().map(|e|e.to_string())}));
                });
                Ok(json!({"operation":id}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrade_confirmation_belongs_to_the_requested_release() {
        let old = json!({"update":{"status":{"version":"1.0","phase":"complete"}}});
        assert!(upgrade_status(&old, "2.0").is_none());
        assert!(upgrade_status(&Value::Null, "2.0").is_none());
        let current = json!({"update":{"status":{"version":"2.0","phase":"complete"}}});
        assert_eq!(
            upgrade_status(&current, "2.0").unwrap()["phase"],
            "complete"
        );
    }
    #[test]
    fn restart_keeps_upgrade_outcome_uncertain_without_a_permanent_busy_flag() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::store::ClientStore::open(root.path()).unwrap();
        store
            .save_node(&crate::store::SavedNode {
                id: "node".into(),
                name: "fixture".into(),
                url: String::new(),
                token: None,
                local: false,
                mesh: None,
                group: None,
            })
            .unwrap();
        store
            .put(
                "node",
                "node-operation",
                &json!({"id":"previous","version":"2.0","running":true}),
            )
            .unwrap();
        let _client = Client::open(root.path()).unwrap();
        let recovered: Value = store.get("node", "node-operation").unwrap().unwrap();
        assert_eq!(recovered["id"], "previous");
        assert_eq!(recovered["version"], "2.0");
        assert_eq!(recovered["running"], false);
        assert_eq!(recovered["completed"], false);
        assert_eq!(recovered["uncertain"], true);
    }
}
