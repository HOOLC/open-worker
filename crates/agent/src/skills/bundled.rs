//! Release selection is metadata; every advertised skill and resource is a file.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use zork_config::skill_bundles::{valid_component, BundleSelection, BundleState};

const MANIFEST: &str = ".bundle-manifest.json";
pub struct BundleFile<'a> {
    pub path: &'a str,
    pub content: &'a [u8],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    revision: String,
    files: BTreeMap<String, String>,
    skills: Vec<String>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn manifest(files: &[BundleFile<'_>]) -> Result<Manifest> {
    ensure!(!files.is_empty(), "empty skill distribution");
    let mut inventory = BTreeMap::new();
    let mut skills = BTreeSet::new();
    for file in files {
        let parts: Vec<_> = file.path.split('/').collect();
        ensure!(
            parts.len() >= 2
                && parts.iter().all(|s| valid_component(s))
                && !file.path.contains('\\'),
            "invalid bundle path"
        );
        ensure!(
            inventory
                .insert(file.path.to_owned(), hash(file.content))
                .is_none(),
            "duplicate bundle path"
        );
        skills.insert(parts[0].to_owned());
        if parts.len() == 2 && parts[1] == "SKILL.md" {
            ensure!(
                file.content.len() as u64 <= MAX_FILE_BYTES,
                "bundled SKILL.md exceeds size limit"
            );
            metadata(std::str::from_utf8(file.content)?)?;
        }
    }
    ensure!(
        skills
            .iter()
            .all(|id| inventory.contains_key(&format!("{id}/SKILL.md"))),
        "each bundled skill needs SKILL.md"
    );
    Ok(Manifest {
        revision: hash(&serde_json::to_vec(&inventory)?),
        files: inventory,
        skills: skills.into_iter().collect(),
    })
}
fn read_manifest(directory: &Path) -> Result<Manifest> {
    ensure!(
        fs::symlink_metadata(directory)?.is_dir(),
        "release must be a real directory"
    );
    let manifest: Manifest = serde_json::from_slice(&fs::read(directory.join(MANIFEST))?)?;
    ensure!(
        manifest.files.keys().all(|path| {
            path.split('/').count() >= 2
                && path.split('/').all(valid_component)
                && !path.contains('\\')
        }),
        "invalid release inventory path"
    );
    let ids: BTreeSet<_> = manifest
        .files
        .keys()
        .filter_map(|path| path.split('/').next().map(str::to_owned))
        .collect();
    ensure!(
        !ids.is_empty()
            && ids
                .iter()
                .all(|id| manifest.files.contains_key(&format!("{id}/SKILL.md")))
            && ids.into_iter().collect::<Vec<_>>() == manifest.skills,
        "invalid release skill IDs"
    );
    ensure!(
        hash(&serde_json::to_vec(&manifest.files)?) == manifest.revision,
        "invalid release inventory hash"
    );
    Ok(manifest)
}
fn inventory(
    directory: &Path,
    relative: &Path,
    result: &mut BTreeMap<String, String>,
) -> Result<()> {
    for entry in fs::read_dir(directory.join(relative))? {
        let entry = entry?;
        if relative.as_os_str().is_empty() && entry.file_name() == MANIFEST {
            continue;
        }
        let path = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            inventory(directory, &path, result)?;
        } else {
            ensure!(kind.is_file(), "bundle contains a non-regular resource");
            let key = path
                .to_str()
                .context("non-UTF-8 bundle path")?
                .replace('\\', "/");
            result.insert(key, hash_file(&entry.path())?);
        }
    }
    Ok(())
}
fn intact(directory: &Path, manifest: &Manifest) -> bool {
    let mut files = BTreeMap::new();
    inventory(directory, Path::new(""), &mut files).is_ok()
        && files == manifest.files
        && serde_json::to_vec(&files).is_ok_and(|bytes| hash(&bytes) == manifest.revision)
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_file_name(format!(".state-{}.tmp", ulid::Ulid::new()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        sync_directory(path.parent().context("state file parent")?)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}
fn save(root: &Path, state: &BundleState) -> Result<()> {
    atomic_json(&root.join(".state.json"), state)
}
fn lock(root: &Path) -> Result<fs::File> {
    fs::create_dir_all(root)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".update.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}
fn writable(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
fn freeze(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            freeze(&path)?;
        } else {
            let mut p = fs::metadata(&path)?.permissions();
            p.set_readonly(true);
            fs::set_permissions(path, p)?;
        }
    }
    sync_directory(directory)
}
fn remove_stage(directory: &Path) {
    if let Ok(entries) = fs::read_dir(directory) {
        let _ = writable(directory);
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                remove_stage(&entry.path());
            } else if kind.is_symlink() {
                if fs::remove_file(entry.path()).is_err() {
                    #[cfg(windows)]
                    let _ = fs::remove_dir(entry.path());
                }
            } else {
                let _ = writable(&entry.path());
                let _ = fs::remove_file(entry.path());
            }
        }
        let _ = fs::remove_dir(directory);
    }
}
/// Prepare and verify a complete immutable directory, then atomically publish
/// its selection. Previous releases remain readable for rollback and old paths.
pub fn install(data_root: &Path, files: &[BundleFile<'_>]) -> Result<String> {
    let incoming = manifest(files)?;
    let root = data_root.join("bundled-skills");
    let custom = data_root.join("custom-skills");
    let _lock = lock(&root)?;
    fs::create_dir_all(&custom)?;
    fs::create_dir_all(root.join(".versions"))?;
    let mut state = zork_config::skill_bundles::load(&root)?;
    if let Some(active) = &state.active {
        let directory = root.join(".versions").join(&active.directory);
        let current = read_manifest(&directory).ok();
        if current.as_ref().is_some_and(|m| intact(&directory, m)) {
            let held = state.rollback
                && state.distribution_revision.as_deref() == Some(&incoming.revision);
            if current.as_ref() == Some(&incoming) || held {
                let selected = active.directory.clone();
                if state.distribution_revision.as_deref() != Some(&incoming.revision) {
                    state.distribution_revision = Some(incoming.revision.clone());
                    state.rollback = false;
                    save(&root, &state)?;
                }
                return Ok(selected);
            }
        }
    }
    let mut selected = None;
    for entry in fs::read_dir(root.join(".versions"))? {
        let entry = entry?;
        if read_manifest(&entry.path()).is_ok_and(|m| m == incoming && intact(&entry.path(), &m)) {
            selected = Some(entry.file_name().to_string_lossy().into_owned());
            break;
        }
    }
    let selected = match selected {
        Some(id) => id,
        None => {
            let id = format!("{}-{}", &incoming.revision[..12], ulid::Ulid::new());
            let stage = root.join(format!(".stage-{}", ulid::Ulid::new()));
            let result = (|| -> Result<()> {
                fs::create_dir(&stage)?;
                for file in files {
                    let path = stage.join(file.path);
                    fs::create_dir_all(path.parent().context("bundle file parent")?)?;
                    let mut output = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?;
                    output.write_all(file.content)?;
                    output.sync_all()?;
                }
                atomic_json(&stage.join(MANIFEST), &incoming)?;
                ensure!(intact(&stage, &incoming), "bundle verification failed");
                freeze(&stage)?;
                fs::rename(&stage, root.join(".versions").join(&id))?;
                Ok(())
            })();
            if result.is_err() {
                remove_stage(&stage);
            }
            result?;
            id
        }
    };
    state.active = Some(BundleSelection {
        directory: selected.clone(),
        skills: incoming.skills,
    });
    state.distribution_revision = Some(incoming.revision);
    state.rollback = false;
    save(&root, &state)?;
    Ok(selected)
}

pub fn is_managed(path: &Path) -> bool {
    path.ancestors()
        .any(|directory| directory.join(MANIFEST).is_file())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    List,
    Disable { skill: String },
    Enable { skill: String },
    Rollback { version: String },
}

pub fn manage(data_root: &Path, request: Request) -> Result<serde_json::Value> {
    let root = data_root.join("bundled-skills");
    let _lock = lock(&root)?;
    let mut state = zork_config::skill_bundles::load(&root)?;
    match request {
        Request::List => {}
        Request::Disable { skill } => {
            ensure!(
                state
                    .active
                    .as_ref()
                    .is_some_and(|active| active.skills.contains(&skill)),
                "unknown bundled skill ID"
            );
            state.disabled.insert(skill);
            save(&root, &state)?;
        }
        Request::Enable { skill } => {
            ensure!(
                state.disabled.contains(&skill)
                    || state
                        .active
                        .as_ref()
                        .is_some_and(|active| active.skills.contains(&skill)),
                "unknown bundled skill ID"
            );
            state.disabled.remove(&skill);
            save(&root, &state)?;
        }
        Request::Rollback { version } => {
            ensure!(valid_component(&version), "invalid bundle version");
            let directory = root.join(".versions").join(&version);
            let manifest = read_manifest(&directory)?;
            ensure!(
                intact(&directory, &manifest),
                "rollback version failed integrity verification"
            );
            state.active = Some(BundleSelection {
                directory: version,
                skills: manifest.skills,
            });
            state.rollback = true;
            save(&root, &state)?;
        }
    }
    status(&root, &state)
}
fn status(root: &Path, state: &BundleState) -> Result<serde_json::Value> {
    let mut versions = Vec::new();
    if root.join(".versions").exists() {
        for entry in fs::read_dir(root.join(".versions"))? {
            let entry = entry?;
            if let Ok(manifest) = read_manifest(&entry.path()) {
                versions.push(json!({"version":entry.file_name().to_string_lossy(),"revision":manifest.revision,"skills":manifest.skills,"intact":intact(&entry.path(),&manifest)}));
            }
        }
    }
    versions.sort_by(|a, b| a["version"].as_str().cmp(&b["version"].as_str()));
    Ok(
        json!({"active":state.active,"disabled":state.disabled,"rollback":state.rollback,"versions":versions}),
    )
}

pub fn register(registry: &crate::session::tools::ToolRegistry, data_root: PathBuf) -> Result<()> {
    use crate::session::tools::{NoToolState, ToolContract, ToolInstance, ToolVersion};
    let description = "Manage this node's release-provided skill files. action=list shows versions and disabled IDs; disable/enable with skill changes visibility for all agents on this node without deleting files; rollback with version selects a complete saved release. Custom copies are unchanged. A rollback persists across restart until a different distribution revision is shipped.";
    registry.register(Arc::new(ToolInstance::new(ToolContract {
        name:"skill.bundle".into(),version:ToolVersion::new("1")?,initial_description:description.into(),detailed_description:description.into(),
        input_schema:json!({"type":"object","properties":{"action":{"type":"string","enum":["list","disable","enable","rollback"]},"skill":{"type":"string"},"version":{"type":"string"}},"required":["action"],"additionalProperties":false}),
    },Arc::new(BundleTool {data_root}),Arc::new(NoToolState))?));
    Ok(())
}
struct BundleTool {
    data_root: PathBuf,
}
impl crate::session::tools::ToolImplementation for BundleTool {
    fn execute<'a>(
        &'a self,
        _context: &'a crate::session::tools::ToolContext,
        arguments: &'a serde_json::Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::session::tools::ToolExecution> + Send + 'a>,
    > {
        let root = self.data_root.clone();
        let arguments = arguments.clone();
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || {
                manage(&root, serde_json::from_value(arguments)?)
            })
            .await;
            match result {
                Ok(Ok(value)) => crate::session::tools::ToolExecution::success(value),
                error => {
                    let message = match error {
                        Ok(Err(e)) => e.to_string(),
                        Err(e) => e.to_string(),
                        _ => unreachable!(),
                    };
                    let mut result =
                        crate::session::tools::ToolExecution::success(json!({"error":message}));
                    result.outcome = crate::session::events::ToolOutcome::Failed;
                    result
                }
            }
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn bundle_cleanup_never_follows_resource_symlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let outside = temporary.path().join("outside");
        let stage = temporary.path().join("stage");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(outside.join("keep.txt"), "preserve").unwrap();
        std::os::unix::fs::symlink(&outside, stage.join("linked-resource")).unwrap();
        super::remove_stage(&stage);
        assert!(!stage.exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "preserve"
        );
    }
}
