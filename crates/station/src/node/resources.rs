//! Read-only inspection of registered resources and an Agent's actual skill catalog.
use super::{authorized, error, NodeState};
use anyhow::{ensure, Context, Result};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::{Component, Path as FsPath},
};
use zork_client_types::resources::*;

fn denied() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "Node administrator token required",
    )
}
fn reply<T: serde::Serialize>(result: Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
fn catalog(state: &NodeState, id: &str) -> Result<(AgentSkills, Vec<zork_agent::skills::Skill>)> {
    let agent = state.app.db.node_agent(id)?.context("Agent not found")?;
    let root = &state.app.config.data_root;
    let sources = zork_config::load_config(root)?
        .skills
        .sources(root, &agent.skill_paths)?;
    let catalog = zork_agent::skills::discover(&sources);
    let skills = catalog
        .skills
        .iter()
        .map(|s| SkillEntry {
            id: blake3::hash(s.path.to_string_lossy().as_bytes())
                .to_hex()
                .to_string(),
            name: s.name.clone(),
            description: s.description.clone(),
            path: s.path.to_string_lossy().into_owned(),
            source: s.source.to_string_lossy().into_owned(),
            content_hash: s.content_hash.clone(),
        })
        .collect();
    Ok((
        AgentSkills {
            skills,
            diagnostics: catalog.diagnostics,
        },
        catalog.skills,
    ))
}
pub(super) async fn skills(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return denied();
    }
    reply(
        tokio::task::spawn_blocking(move || catalog(&state, &id).map(|(public, _)| public))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|r| r),
    )
}
#[derive(Default, Deserialize)]
pub(super) struct ReadQuery {
    file: Option<String>,
    log: Option<String>,
}
pub(super) async fn skill(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path((id, skill)): Path<(String, String)>,
    Query(query): Query<ReadQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return denied();
    }
    reply(
        tokio::task::spawn_blocking(move || -> Result<ResourceDetails> {
            let (public, catalog) = catalog(&state, &id)?;
            let position = public
                .skills
                .iter()
                .position(|entry| entry.id == skill)
                .context("Skill is no longer in this Agent's catalog")?;
            let selected = &catalog[position];
            let root = selected.path.parent().context("Skill directory missing")?;
            let file = query.file.as_deref().unwrap_or("SKILL.md");
            let mut document = read_document(root, file)?;
            if query.file.is_none() {
                document.text = skill_body(&document.text).to_owned();
            }
            let mut files = Vec::new();
            let mut budget = 256;
            list_files(root, root, 0, &mut budget, &mut files)?;
            files.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(ResourceDetails {
                title: selected.name.clone(),
                description: selected.description.clone(),
                document: Some(document),
                files,
                facts: vec![
                    (
                        "source".into(),
                        selected.source.to_string_lossy().into_owned(),
                    ),
                    ("path".into(), selected.path.to_string_lossy().into_owned()),
                    ("files_truncated".into(), (budget == 0).to_string()),
                ],
                ..Default::default()
            })
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|r| r),
    )
}
fn skill_body(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return text;
    };
    if first.trim_start_matches('\u{feff}').trim() != "---" {
        return text;
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if line.trim() == "---" {
            return &text[offset..];
        }
    }
    text
}
fn read_document(root: &FsPath, relative: &str) -> Result<ResourceDocument> {
    let requested = FsPath::new(relative);
    ensure!(
        !relative.is_empty()
            && relative.len() <= 1024
            && !requested.is_absolute()
            && requested
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Invalid resource file path"
    );
    let root = root.canonicalize()?;
    let mut path = root.clone();
    for part in requested.components() {
        path.push(part);
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Resource symlinks cannot be read"
        );
    }
    ensure!(
        path.canonicalize()?.starts_with(&root),
        "Resource file is outside the Skill"
    );
    let file = fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "Resource is not a regular file");
    let mut bytes = Vec::new();
    file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > 128 * 1024;
    if truncated {
        bytes.truncate(128 * 1024);
    }
    ensure!(!bytes.contains(&0), "This resource is not a text file");
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error)
            if truncated && error.utf8_error().valid_up_to() + 4 >= error.as_bytes().len() =>
        {
            let end = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..end].to_vec())?
        }
        Err(_) => anyhow::bail!("This resource is not UTF-8 text"),
    };
    Ok(ResourceDocument {
        path: relative.into(),
        text,
        truncated,
    })
}
fn list_files(
    root: &FsPath,
    directory: &FsPath,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<ResourceFile>,
) -> Result<()> {
    if depth > 6 || *budget == 0 {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if entry.file_name() != ".git" {
                list_files(root, &path, depth + 1, budget, out)?;
            }
        } else if kind.is_file() && path != root.join("SKILL.md") {
            out.push(ResourceFile {
                path: path.strip_prefix(root)?.to_string_lossy().into_owned(),
                byte_len: entry.metadata()?.len(),
            });
        }
    }
    Ok(())
}
pub(super) async fn mcp(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return denied();
    }
    reply(crate::mcp::client_details(&state.app, &id).await)
}
pub(super) async fn service(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ReadQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return denied();
    }
    let Some(mesh) = state.app.mesh.get().cloned() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Mesh is not ready");
    };
    reply(
        tokio::task::spawn_blocking(move || {
            mesh.services.client_details(&id, query.log.as_deref())
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|r| r),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skill_reader_starts_at_instructions_and_preserves_body_rules() {
        let text="\u{feff}---\r\nname: research\r\ndescription: notes\r\n---\r\n# 研究\r\n\r\n正文\r\n---\r\n附录";
        assert_eq!(skill_body(text), "# 研究\r\n\r\n正文\r\n---\r\n附录");
        assert_eq!(skill_body("# 普通资源\n正文"), "# 普通资源\n正文");
    }
    #[test]
    fn skill_resources_cannot_escape_the_selected_package() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("SKILL.md"), "instructions").unwrap();
        assert_eq!(
            read_document(root.path(), "SKILL.md").unwrap().text,
            "instructions"
        );
        assert!(read_document(root.path(), "../secret").is_err());
        assert!(read_document(root.path(), "/etc/hosts").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hosts", root.path().join("linked")).unwrap();
            assert!(read_document(root.path(), "linked").is_err());
        }
        fs::write(root.path().join("binary"), [0, 1, 2]).unwrap();
        assert!(read_document(root.path(), "binary").is_err());
    }
}
