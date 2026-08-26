use std::path::PathBuf;

use serde_json::Value;

use crate::session::runtime::{
    ModelLimits, ProfileExecution, ProfileResolveError, ProfileResolver,
};
use crate::session::state::SessionSelection;

#[derive(Clone)]
pub struct ProfileStore {
    data_root: PathBuf,
    fake: bool,
    no_streaming: bool,
    http: reqwest::Client,
    statuses: zork_profile::MemoryStore,
}

impl ProfileStore {
    pub fn open(data_root: PathBuf, fake: bool, no_streaming: bool) -> Self {
        Self {
            data_root,
            fake,
            no_streaming,
            http: reqwest::Client::new(),
            statuses: zork_profile::MemoryStore::default(),
        }
    }

    fn paths(&self) -> zork_profile::DataRootPaths {
        zork_profile::DataRootPaths {
            data_root: self.data_root.clone(),
        }
    }

    pub fn list(&self) -> anyhow::Result<Vec<zork_profile::ProfileView>> {
        zork_profile::list_profiles_with_status(&self.paths(), &self.statuses)
    }

    pub fn get(&self, profile_id: &str) -> anyhow::Result<Option<zork_profile::ProfileView>> {
        zork_profile::get_profile_with_status(&self.paths(), &self.statuses, profile_id)
    }

    pub fn put(&self, profile_id: &str, body: Value) -> anyhow::Result<zork_profile::ProfileView> {
        let profile = zork_profile::put_profile(&self.paths(), profile_id, body)?;
        zork_profile::ProfileStore::remove_probe(&self.statuses, profile_id)?;
        Ok(profile)
    }

    pub fn delete(&self, profile_id: &str) -> anyhow::Result<()> {
        zork_profile::delete_profile(&self.paths(), profile_id)?;
        zork_profile::ProfileStore::remove_probe(&self.statuses, profile_id)
    }

    pub async fn refresh_all_statuses(&self) -> anyhow::Result<()> {
        if !self.fake {
            zork_profile::refresh_all(&self.paths(), &self.statuses, &self.http).await?;
        }
        Ok(())
    }

    pub async fn refresh_status(&self, profile_id: &str) -> anyhow::Result<()> {
        if !self.fake {
            zork_profile::refresh_profile(&self.paths(), &self.statuses, &self.http, profile_id)
                .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ProfileResolver for ProfileStore {
    fn model_limits(
        &self,
        selection: &SessionSelection,
    ) -> Result<Option<ModelLimits>, ProfileResolveError> {
        let paths = self.paths();
        let document = zork_profile::read_profile(&paths, &selection.profile_id)
            .map_err(|_| ProfileResolveError::NotFound)?;
        let model = zork_profile::select_model(&document, &selection.model, &selection.thinking)
            .map_err(|_| ProfileResolveError::InvalidSelection)?;
        Ok(model.limits.as_ref().map(|limits| ModelLimits {
            context_window_tokens: limits.context_window_tokens,
            max_output_tokens: limits.max_output_tokens,
        }))
    }

    async fn resolve(
        &self,
        selection: &SessionSelection,
    ) -> Result<ProfileExecution, ProfileResolveError> {
        if self.fake {
            return Ok(ProfileExecution::new(
                selection.profile_id.clone(),
                "openai".to_owned(),
                selection.model.clone(),
                "openai-completions".to_owned(),
                !self.no_streaming,
                false,
                None,
                "http://127.0.0.1:9/v1".to_owned(),
                Default::default(),
                selection.thinking.clone(),
                "fake".to_owned(),
            ));
        }
        let execution = zork_profile::load_selected(
            &self.paths(),
            &self.http,
            &selection.profile_id,
            &selection.model,
            &selection.thinking,
        )
        .await
        .map_err(|_| ProfileResolveError::AuthUnavailable)?;
        Ok(ProfileExecution::new(
            execution.profile_id,
            execution.provider.clone(),
            execution.model,
            execution.api,
            execution.streaming && !self.no_streaming,
            execution.parallel_tool_calls,
            execution.service_tier,
            execution.base_url,
            execution.headers,
            execution.thinking,
            execution.bearer,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_override_disables_streaming_for_every_resolved_profile() {
        let selection = SessionSelection {
            profile_id: "fixture".to_owned(),
            model: "model".to_owned(),
            thinking: "high".to_owned(),
        };

        let enabled = ProfileStore::open(PathBuf::new(), true, false)
            .resolve(&selection)
            .await
            .unwrap();
        assert!(enabled.streaming());

        let disabled = ProfileStore::open(PathBuf::new(), true, true)
            .resolve(&selection)
            .await
            .unwrap();
        assert!(!disabled.streaming());
    }
}
