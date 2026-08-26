use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde_json::Value;

type ProbeMap = HashMap<String, (String, Value, Value)>;

pub trait ProfilePaths {
    fn profiles_root(&self) -> PathBuf;
}

pub trait ProfileStore {
    fn list_probes(&self) -> Result<Vec<(String, String, Value, Value)>>;
    fn upsert_probe(&self, profile_id: &str, account: &Value, rate_limits: &Value) -> Result<()>;
    fn remove_probe(&self, profile_id: &str) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct DataRootPaths {
    pub data_root: PathBuf,
}

impl ProfilePaths for DataRootPaths {
    fn profiles_root(&self) -> PathBuf {
        self.data_root.join("profiles")
    }
}

impl<T: ProfileStore> ProfileStore for Arc<T> {
    fn list_probes(&self) -> Result<Vec<(String, String, Value, Value)>> {
        (**self).list_probes()
    }

    fn upsert_probe(&self, profile_id: &str, account: &Value, rate_limits: &Value) -> Result<()> {
        (**self).upsert_probe(profile_id, account, rate_limits)
    }

    fn remove_probe(&self, profile_id: &str) -> Result<()> {
        (**self).remove_probe(profile_id)
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<Mutex<ProbeMap>>,
}

impl ProfileStore for MemoryStore {
    fn list_probes(&self) -> Result<Vec<(String, String, Value, Value)>> {
        let guard = self.inner.lock().expect("profile status store");
        Ok(guard
            .iter()
            .map(|(profile_id, (checked_at, account, rate_limits))| {
                (
                    profile_id.clone(),
                    checked_at.clone(),
                    account.clone(),
                    rate_limits.clone(),
                )
            })
            .collect())
    }

    fn upsert_probe(&self, profile_id: &str, account: &Value, rate_limits: &Value) -> Result<()> {
        let checked_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.inner.lock().expect("profile status store").insert(
            profile_id.to_owned(),
            (checked_at, account.clone(), rate_limits.clone()),
        );
        Ok(())
    }

    fn remove_probe(&self, profile_id: &str) -> Result<()> {
        self.inner
            .lock()
            .expect("profile status store")
            .remove(profile_id);
        Ok(())
    }
}

pub(crate) fn profile_path(paths: &impl ProfilePaths, profile_id: &str) -> Result<PathBuf> {
    validate_profile_id(profile_id)?;
    Ok(paths.profiles_root().join(format!("{profile_id}.json")))
}

pub(crate) fn validate_profile_id(profile_id: &str) -> Result<()> {
    if profile_id.is_empty()
        || profile_id == "auto"
        || profile_id.contains('/')
        || profile_id.contains('\\')
        || profile_id.contains("..")
        || profile_id.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
        })
    {
        anyhow::bail!("invalid profile id");
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("profile path has no parent")?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("profile path has no UTF-8 file name")?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("create {}", temporary.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_selector_value_is_not_a_profile_id() {
        assert_eq!(
            validate_profile_id("auto").unwrap_err().to_string(),
            "invalid profile id"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_profile_write_is_private_and_uses_a_unique_temporary_path() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.json");
        let old_fixed_temporary = path.with_extension("json.tmp");
        fs::write(&old_fixed_temporary, b"stale").unwrap();

        atomic_write(&path, b"secret").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"secret");
        assert_eq!(fs::read(&old_fixed_temporary).unwrap(), b"stale");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
