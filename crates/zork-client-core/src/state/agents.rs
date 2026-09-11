use super::{Device, Domains};
use serde_json::{json, Value};

impl Device {
    pub async fn open_agent(&self, id: &str) -> anyhow::Result<Value> {
        let result = self
            .client
            .node_request(
                reqwest::Method::POST,
                format!("/v1/node/agents/{id}/open"),
                Some(json!({})),
            )
            .await?;
        self.refresh(Domains::AGENTS | Domains::SESSIONS | Domains::TASKS)
            .await;
        Ok(result)
    }
}
