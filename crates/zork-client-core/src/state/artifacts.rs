use super::Device;

impl Device {
    /// Immutable artifact bytes and offline fallback share the same core cache
    /// on every client. Decoding images and choosing save locations remain UI work.
    pub async fn artifact_content(&self, id: &str) -> Result<Vec<u8>, crate::api::ApiError> {
        if let Some((store, node)) = &self.cache {
            if let Ok(Some(bytes)) = store.blob(node, &format!("upload:{id}")) {
                return Ok(bytes);
            }
        }
        let result = self.client.artifact_content(id).await;
        let Some((store, node)) = &self.cache else {
            return result;
        };
        match result {
            Ok(bytes) => {
                let _ = store.put_blob(node, id, &bytes);
                Ok(bytes)
            }
            Err(error) => match store.blob(node, id) {
                Ok(Some(bytes)) => Ok(bytes),
                _ => Err(error),
            },
        }
    }
}
