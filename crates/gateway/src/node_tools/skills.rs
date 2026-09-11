use super::*;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    content: String,
    #[serde(default)]
    resources: Vec<Resource>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    path: String,
    base64: String,
    #[serde(default)]
    executable: bool,
}
fn root(state: &AppState) -> Result<PathBuf> {
    let root = state.config.data_root.join("managed-skills");
    fs::create_dir_all(&root)?;
    Ok(root.canonicalize()?)
}
fn path(state: &AppState, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    let path = root(state)?.join(id);
    ensure!(
        fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()),
        "skill_not_installed"
    );
    Ok(path)
}
fn relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', ':'])
        && value.split('/').all(|p| {
            !p.is_empty()
                && !p.starts_with('.')
                && !p.ends_with(['.', ' '])
                && !p.chars().any(char::is_control)
        })
}
fn validate(package: &Package) -> Result<Value> {
    let metadata = zork_agent::skills::management::validate(&package.content)
        .map_err(|_| anyhow::anyhow!("skill_invalid_manifest"))?;
    ensure!(
        package.resources.len() <= 32 && serde_json::to_vec(package)?.len() <= 96 * 1024,
        "skill_package_limit"
    );
    let mut paths = BTreeSet::new();
    for resource in &package.resources {
        ensure!(
            relative(&resource.path)
                && resource.path != "SKILL.md"
                && paths.insert(resource.path.clone()),
            "skill_invalid_resource_path"
        );
        use base64::Engine;
        ensure!(
            base64::engine::general_purpose::STANDARD
                .decode(&resource.base64)
                .is_ok(),
            "skill_invalid_resource_data"
        );
    }
    Ok(metadata)
}
fn package(directory: &Path) -> Result<Package> {
    let directory = directory.canonicalize()?;
    fn walk(
        root: &Path,
        relative: &Path,
        files: &mut Vec<Resource>,
        entries: &mut usize,
        bytes: &mut usize,
    ) -> Result<()> {
        ensure!(relative.components().count() <= 8, "skill_package_limit");
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            *entries += 1;
            ensure!(*entries <= 128, "skill_package_limit");
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let rel = relative.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(root, &rel, files, entries, bytes)?;
            } else {
                ensure!(kind.is_file(), "skill_symlink_resource");
                if rel == Path::new("SKILL.md") {
                    continue;
                }
                ensure!(
                    entry.metadata()?.len() <= 96 * 1024 && files.len() < 32,
                    "skill_package_limit"
                );
                let data = fs::read(entry.path())?;
                *bytes += data.len();
                ensure!(*bytes <= 96 * 1024, "skill_package_limit");
                #[cfg(unix)]
                let executable = {
                    use std::os::unix::fs::PermissionsExt;
                    entry.metadata()?.permissions().mode() & 0o111 != 0
                };
                #[cfg(not(unix))]
                let executable = false;
                use base64::Engine;
                files.push(Resource {
                    path: rel.to_string_lossy().replace('\\', "/"),
                    base64: base64::engine::general_purpose::STANDARD.encode(data),
                    executable,
                });
            }
        }
        Ok(())
    }
    let manifest = directory.join("SKILL.md");
    ensure!(
        fs::symlink_metadata(&manifest).is_ok_and(|m| m.is_file() && m.len() <= 128 * 1024),
        "skill_invalid_manifest"
    );
    let mut result = Package {
        content: fs::read_to_string(manifest)?,
        resources: Vec::new(),
    };
    walk(
        &directory,
        Path::new(""),
        &mut result.resources,
        &mut 0,
        &mut 0,
    )?;
    result.resources.sort_by(|a, b| a.path.cmp(&b.path));
    validate(&result)?;
    Ok(result)
}
fn descriptor(state: &AppState, id: &str) -> Result<Value> {
    let path = path(state, id)?;
    let package = package(&path)?;
    let meta = validate(&package)?;
    Ok(
        json!({"target":node_access::identity(state),"skill_id":id,"name":meta["name"],"description":meta["description"],"revision":fingerprint(&package)?,"path":path,"resource_count":package.resources.len()}),
    )
}
fn expected(state: &AppState, args: &Value) -> Result<(String, PathBuf)> {
    let id = field(args, "skill_id")?;
    let current = descriptor(state, id)?;
    ensure!(
        args["expected_revision"] == current["revision"],
        "skill_revision_conflict"
    );
    Ok((id.into(), path(state, id)?))
}
fn bindings(state: &AppState, path: &Path) -> Result<Vec<Value>> {
    Ok(state
        .db
        .node_agents()?
        .iter()
        .filter(|a| a.skill_paths.iter().any(|p| p == path))
        .map(|a| json!({"agent_id":a.id,"name":a.name}))
        .collect())
}
pub(crate) fn client_resources(
    state: &AppState,
) -> Result<Vec<zork_client_types::resources::Resource>> {
    use zork_client_types::resources::{Resource, ResourceKind, ResourceSubject};
    let _gate = state.node_tools.gate.lock().expect("managed skills");
    let root = state.config.data_root.join("managed-skills");
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut items = vec![];
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if valid_id(&id).is_err() || !entry.file_type()?.is_dir() {
            continue;
        }
        let data = descriptor(state, &id)?;
        let subjects = bindings(
            state,
            Path::new(data["path"].as_str().context("skill_invalid_path")?),
        )?
        .iter()
        .map(|agent| ResourceSubject {
            id: agent["agent_id"].as_str().unwrap_or_default().into(),
            name: agent["name"].as_str().unwrap_or_default().into(),
            origin: None,
        })
        .collect::<Vec<_>>();
        let mut item = Resource::new(
            ResourceKind::Skill,
            id,
            data["name"].as_str().unwrap_or_default().into(),
            "installed".into(),
            if subjects.is_empty() {
                "unbound"
            } else {
                "bound"
            }
            .into(),
        );
        item.description = data["description"].as_str().unwrap_or_default().into();
        item.path = data["path"].as_str().map(str::to_owned);
        item.revision = data["revision"].as_str().map(str::to_owned);
        item.resource_count = data["resource_count"].as_u64().map(|count| count as usize);
        item.subjects = subjects;
        items.push(item);
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(items)
}
pub fn read(state: &AppState, rpc: &Rpc) -> Result<Value> {
    let _gate = state.node_tools.gate.lock().expect("managed skills");
    match rpc.tool.as_str() {
        "skill.installed" => {
            let mut ids = fs::read_dir(root(state)?)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                .filter(|name| valid_id(name).is_ok())
                .collect::<Vec<_>>();
            ids.sort();
            let start = rpc
                .arguments
                .get("cursor")
                .and_then(Value::as_str)
                .unwrap_or("");
            let mut selected = ids.into_iter().filter(|id| id.as_str() > start);
            let mut items = Vec::new();
            for id in selected.by_ref().take(20) {
                items.push(descriptor(state, &id)?);
            }
            let next = if selected.next().is_some() {
                items
                    .last()
                    .and_then(|v| v["skill_id"].as_str())
                    .map(str::to_owned)
            } else {
                None
            };
            Ok(json!({"target":node_access::identity(state),"items":items,"next_cursor":next}))
        }
        "skill.export" => {
            let id = field(&rpc.arguments, "skill_id")?;
            let descriptor = descriptor(state, id)?;
            if let Some(revision) = rpc.arguments.get("expected_revision") {
                ensure!(
                    *revision == descriptor["revision"],
                    "skill_revision_conflict"
                );
            }
            Ok(json!({"skill":descriptor,"package":package(&path(state,id)?)?}))
        }
        "skill.bindings" => {
            let id = field(&rpc.arguments, "skill_id")?;
            valid_id(id)?;
            let root = root(state)?;
            let active = root.join(id);
            let package_state = if fs::symlink_metadata(&active).is_ok_and(|m| m.is_dir()) {
                "installed"
            } else {
                ensure!(
                    fs::symlink_metadata(root.join(".archive").join(id)).is_ok_and(|m| m.is_dir()),
                    "skill_not_installed"
                );
                "archived"
            };
            // Bindings refer to the original active path even after archival.
            // Do not require that path to still exist when auditing references.
            Ok(json!({"target":node_access::identity(state),"skill_id":id,
                "package_state":package_state,"agents":bindings(state,&active)?}))
        }
        _ => anyhow::bail!("skill_unknown_operation"),
    }
}
pub fn mutate(state: &AppState, rpc: &Rpc, operation: &str) -> Result<Value> {
    let _gate = state.node_tools.gate.lock().expect("managed skills");
    match rpc.tool.as_str() {
        "skill.install" | "skill.import" => {
            let content = if rpc.tool == "skill.import" {
                let p = Path::new(field(&rpc.arguments, "path")?);
                ensure!(p.is_absolute(), "skill_absolute_source_required");
                package(p)?
            } else {
                serde_json::from_value::<Package>(rpc.arguments["package"].clone())
                    .map_err(|_| anyhow::anyhow!("skill_invalid_package"))?
            };
            validate(&content)?;
            let root = root(state)?;
            let dest = root.join(operation);
            ensure!(!dest.exists(), "skill_install_conflict");
            let stage = root.join(format!(".stage-{operation}"));
            node_access::manage(state, &rpc.subject, rpc.subject.origin == "local")?;
            state
                .node_tools
                .store
                .finish(operation, "dispatching", None)?;
            fs::create_dir(&stage)?;
            let result = (|| -> Result<()> {
                fs::write(stage.join("SKILL.md"), &content.content)?;
                for resource in &content.resources {
                    let path = stage.join(&resource.path);
                    fs::create_dir_all(path.parent().context("skill_resource_parent")?)?;
                    use base64::Engine;
                    fs::write(
                        &path,
                        base64::engine::general_purpose::STANDARD.decode(&resource.base64)?,
                    )?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(
                            &path,
                            fs::Permissions::from_mode(if resource.executable {
                                0o755
                            } else {
                                0o644
                            }),
                        )?;
                    }
                }
                fs::rename(&stage, &dest)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_dir_all(&stage);
            }
            result?;
            descriptor(state, operation)
        }
        "skill.bind" | "skill.unbind" => {
            let (id, path) = expected(state, &rpc.arguments)?;
            let agent = field(&rpc.arguments, "agent_id")?;
            state
                .db
                .node_agent(agent)?
                .context("skill_agent_not_found")?;
            node_access::manage(state, &rpc.subject, rpc.subject.origin == "local")?;
            state
                .node_tools
                .store
                .finish(operation, "dispatching", None)?;
            let result = state
                .db
                .bind_managed_skill(agent, &path, rpc.tool == "skill.bind")?;
            Ok(json!({"target":node_access::identity(state),"skill_id":id,"agent":result}))
        }
        "skill.uninstall" => {
            let (id, path) = expected(state, &rpc.arguments)?;
            ensure!(bindings(state, &path)?.is_empty(), "skill_still_bound");
            node_access::manage(state, &rpc.subject, rpc.subject.origin == "local")?;
            state
                .node_tools
                .store
                .finish(operation, "dispatching", None)?;
            let archive = root(state)?.join(".archive");
            fs::create_dir_all(&archive)?;
            fs::rename(&path, archive.join(&id))?;
            Ok(
                json!({"target":node_access::identity(state),"skill_id":id,"removed":true,"resources_preserved":true}),
            )
        }
        _ => anyhow::bail!("skill_unknown_operation"),
    }
}
