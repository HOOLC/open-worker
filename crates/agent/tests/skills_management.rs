use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};
use zork_agent::{
    session::{
        events::ToolOutcome,
        tools::{ToolContext, ToolExecution, ToolRegistry, ToolResolution, ToolVersion},
    },
    skills,
};

fn registry(paths: Vec<std::path::PathBuf>) -> ToolRegistry {
    let registry = ToolRegistry::default();
    let sources: skills::SkillSources = Arc::new(move |_| Ok(paths.clone()));
    skills::register_tools(&registry, sources.clone()).unwrap();
    skills::management::register(&registry, sources, None).unwrap();
    registry
}
async fn call(registry: &ToolRegistry, name: &str, args: Value) -> ToolExecution {
    let ToolResolution::Ready(tool) = registry.resolve(name, Some(&ToolVersion::new("2").unwrap()))
    else {
        panic!("missing tool {name}");
    };
    tool.execute(
        &ToolContext {
            control: None,
            session_id: "current-session".into(),
            invocation_id: "call".into(),
            workspace: "/unused".into(),
        },
        &args,
    )
    .await
}
fn document(body: &str) -> String {
    format!("---\nname: example\ndescription: Use for example tasks\n---\n{body}\n")
}
fn write(root: &Path, body: &str) -> Value {
    json!({"source":root,"directory":"group/example","content":document(body)})
}

#[tokio::test]
async fn create_update_conflict_archive_and_restore_preserve_resources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let registry = registry(vec![root.to_owned()]);
    let created = call(&registry, "skill.write", write(root, "first")).await;
    assert_eq!(
        created.outcome,
        ToolOutcome::Succeeded,
        "{:?}",
        created.data
    );
    let manifest = root.join("group/example/SKILL.md");
    let resource = root.join("group/example/scripts/run.sh");
    fs::create_dir(resource.parent().unwrap()).unwrap();
    fs::write(&resource, "echo preserved").unwrap();
    let refused = call(&registry, "skill.write", write(root, "overwrite")).await;
    assert_eq!(refused.outcome, ToolOutcome::Failed);
    assert_eq!(fs::read_to_string(&manifest).unwrap(), document("first"));
    let mut update = write(root, "second");
    update["expected_hash"] = created.data["content_hash"].clone();
    let updated = call(&registry, "skill.write", update.clone()).await;
    assert_eq!(updated.outcome, ToolOutcome::Succeeded);
    assert_eq!(
        call(&registry, "skill.write", update).await.outcome,
        ToolOutcome::Failed
    );
    // An example SKILL.md under resources must not become active on archive.
    fs::create_dir_all(root.join("group/example/examples/nested")).unwrap();
    fs::write(
        root.join("group/example/examples/nested/SKILL.md"),
        document("not discoverable"),
    )
    .unwrap();
    let archived = call(&registry,"skill.archive",json!({"source":root,"directory":"group/example","expected_hash":updated.data["content_hash"]})).await;
    assert_eq!(
        archived.outcome,
        ToolOutcome::Succeeded,
        "{:?}",
        archived.data
    );
    assert!(!manifest.exists());
    assert_eq!(fs::read_to_string(&resource).unwrap(), "echo preserved");
    assert!(skills::discover(&[root.to_owned()]).skills.is_empty());
    let backup = archived.data["archived_path"].as_str().unwrap();
    assert_eq!(fs::read_to_string(backup).unwrap(), document("second"));
    assert_eq!(
        call(&registry, "skill.write", write(root, "restored"))
            .await
            .outcome,
        ToolOutcome::Succeeded
    );
    assert!(Path::new(backup).exists());
    assert_eq!(skills::discover(&[root.to_owned()]).skills.len(), 1);
}

#[tokio::test]
async fn validation_and_source_boundaries_fail_without_writing_manifests() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let registry = registry(vec![root.to_owned()]);
    assert_eq!(
        call(
            &registry,
            "skill.validate",
            json!({"content":document("instructions")})
        )
        .await
        .outcome,
        ToolOutcome::Succeeded
    );
    for content in [
        "invalid".to_owned(),
        document(""),
        "x".repeat(128 * 1024 + 1),
    ] {
        assert_eq!(
            call(&registry, "skill.validate", json!({"content":content}))
                .await
                .outcome,
            ToolOutcome::Failed
        );
    }
    for dir in [
        "../escaped",
        "/absolute",
        ".hidden/skill",
        "group/../escaped",
    ] {
        let mut args = write(root, "body");
        args["directory"] = json!(dir);
        assert_eq!(
            call(&registry, "skill.write", args).await.outcome,
            ToolOutcome::Failed
        );
    }
    let mut args = write(root, "body");
    args["source"] = json!(root.join("unconfigured"));
    assert_eq!(
        call(&registry, "skill.write", args).await.outcome,
        ToolOutcome::Failed
    );
    assert!(fs::read_dir(root).unwrap().next().is_none());
    assert_eq!(
        call(&registry, "skill.sources", json!({"action":"list"}))
            .await
            .data["editable"],
        false
    );
    assert_eq!(
        call(
            &registry,
            "skill.sources",
            json!({"action":"add","path":"new"})
        )
        .await
        .outcome,
        ToolOutcome::Failed
    );
}

#[tokio::test]
async fn competing_updates_accept_only_one_expected_hash() {
    let temp = tempfile::tempdir().unwrap();
    let registry = registry(vec![temp.path().to_owned()]);
    let initial = call(&registry, "skill.write", write(temp.path(), "first")).await;
    let mut one = write(temp.path(), "one");
    one["expected_hash"] = initial.data["content_hash"].clone();
    let mut two = write(temp.path(), "two");
    two["expected_hash"] = initial.data["content_hash"].clone();
    let (one, two) = tokio::join!(
        call(&registry, "skill.write", one),
        call(&registry, "skill.write", two)
    );
    assert_eq!(
        [one, two]
            .iter()
            .filter(|r| r.outcome == ToolOutcome::Succeeded)
            .count(),
        1
    );
    let content = fs::read_to_string(temp.path().join("group/example/SKILL.md")).unwrap();
    assert!(content == document("one") || content == document("two"));
}

#[tokio::test]
async fn release_managed_sources_reject_mutations_but_custom_copies_allow_them() {
    let temp = tempfile::tempdir().unwrap();
    skills::management::provision_bundled(temp.path()).unwrap();
    let managed = zork_config::skill_bundles::sources(temp.path()).unwrap()[0].clone();
    let root = temp.path().join("custom-skills");
    let registry = registry(vec![managed.clone(), root.clone()]);
    let catalog = skills::discover(&[managed.clone()]);
    let hash = catalog.skills[0].content_hash.clone();
    let result = call(
        &registry,
        "skill.archive",
        json!({"source":managed,"directory":".","expected_hash":hash}),
    )
    .await;
    assert_eq!(result.outcome, ToolOutcome::Failed);
    assert!(result.data["error"]
        .as_str()
        .unwrap()
        .contains("release-managed"));
    assert_eq!(
        call(&registry, "skill.write", write(&root, "customized"))
            .await
            .outcome,
        ToolOutcome::Succeeded
    );
}

#[test]
fn service_sharing_skill_is_shipped_and_discoverable() {
    let temp = tempfile::tempdir().unwrap();
    skills::management::provision_bundled(temp.path()).unwrap();
    let sources = zork_config::skill_bundles::sources(temp.path()).unwrap();
    let catalog = skills::discover(&sources);
    let service = catalog
        .skills
        .iter()
        .find(|skill| skill.name == "service-sharing")
        .expect("service-sharing must be available to Gateway Agents");
    assert!(service
        .path
        .starts_with(temp.path().canonicalize().unwrap()));
    assert!(!skills::read_document(&service.path).unwrap().is_empty());
}
