use std::{
    fs,
    path::{Path, PathBuf},
};
use zork_agent::skills::{
    self,
    bundled::{self, BundleFile, Request},
};

fn doc(text: &str) -> String {
    format!("---\nname: shared-name\ndescription: {text}\n---\n{text}\n")
}
fn install(root: &Path, text: &str) -> String {
    let guide = doc(text);
    let script = format!("echo {text}\n");
    bundled::install(
        root,
        &[
            BundleFile {
                path: "guide/SKILL.md",
                content: guide.as_bytes(),
            },
            BundleFile {
                path: "guide/scripts/run.sh",
                content: script.as_bytes(),
            },
        ],
    )
    .unwrap()
}
fn release(root: &Path, version: &str) -> PathBuf {
    root.join("bundled-skills/.versions").join(version)
}
fn sources(root: &Path) -> Vec<PathBuf> {
    zork_config::skill_bundles::sources(root).unwrap()
}

#[test]
fn complete_updates_keep_old_files_and_rollback_persists_until_a_new_distribution() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = install(root, "first");
    assert_eq!(install(root, "first"), first);
    let second = install(root, "second");
    assert_ne!(first, second);
    assert_eq!(
        fs::read_to_string(sources(root)[0].join("scripts/run.sh")).unwrap(),
        "echo second\n"
    );
    assert_eq!(
        fs::read_to_string(release(root, &first).join("guide/SKILL.md")).unwrap(),
        doc("first")
    );
    bundled::manage(
        root,
        Request::Rollback {
            version: first.clone(),
        },
    )
    .unwrap();
    assert_eq!(install(root, "second"), first);
    assert_eq!(
        fs::read_to_string(sources(root)[0].join("SKILL.md")).unwrap(),
        doc("first")
    );
    let third = install(root, "third");
    assert_ne!(third, first);
    let state = bundled::manage(root, Request::List).unwrap();
    assert_eq!(state["active"]["directory"], third);
    assert_eq!(state["rollback"], false);
    assert_eq!(state["versions"].as_array().unwrap().len(), 3);
}

#[test]
fn disable_survives_updates_restart_and_rollback_without_touching_custom_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = install(root, "first");
    let custom = root.join("custom-skills/my-copy");
    fs::create_dir_all(custom.join("scripts")).unwrap();
    fs::copy(
        sources(root)[0].join("scripts/run.sh"),
        custom.join("scripts/run.sh"),
    )
    .unwrap();
    fs::write(custom.join("SKILL.md"), doc("custom")).unwrap();
    bundled::manage(
        root,
        Request::Disable {
            skill: "guide".into(),
        },
    )
    .unwrap();
    install(root, "second");
    assert!(sources(root).is_empty());
    bundled::manage(root, Request::Rollback { version: first }).unwrap();
    assert!(sources(root).is_empty());
    assert_eq!(
        fs::read_to_string(custom.join("SKILL.md")).unwrap(),
        doc("custom")
    );
    assert_eq!(
        fs::read_to_string(custom.join("scripts/run.sh")).unwrap(),
        "echo first\n"
    );
    let catalog = skills::discover(
        &zork_config::SkillsConfig::default()
            .sources(root, &[])
            .unwrap(),
    );
    assert_eq!(catalog.skills.len(), 1);
    assert_eq!(
        catalog.skills[0].path,
        fs::canonicalize(custom.join("SKILL.md")).unwrap()
    );
    bundled::manage(
        root,
        Request::Enable {
            skill: "guide".into(),
        },
    )
    .unwrap();
    assert_eq!(sources(root).len(), 1);
    assert!(bundled::manage(
        root,
        Request::Disable {
            skill: "unknown".into()
        }
    )
    .is_err());
}

#[test]
fn invalid_release_is_replaced_without_creating_custom_backups() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = install(root, "first");
    let file = release(root, &first).join("guide/scripts/run.sh");
    fs::remove_file(&file).unwrap();
    fs::write(&file, "invalid release resource").unwrap();
    install(root, "second");
    assert!(fs::read_dir(root.join("custom-skills"))
        .unwrap()
        .next()
        .is_none());
    assert_eq!(
        fs::read_to_string(sources(root)[0].join("scripts/run.sh")).unwrap(),
        "echo second\n"
    );
    assert!(bundled::manage(root, Request::Rollback { version: first }).is_err());
}

#[test]
fn invalid_or_partial_releases_never_replace_active_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = install(root, "first");
    for files in [
        vec![BundleFile {
            path: "../escape",
            content: b"bad",
        }],
        vec![BundleFile {
            path: "guide/script.sh",
            content: b"missing manifest",
        }],
        vec![BundleFile {
            path: "guide/SKILL.md",
            content: b"invalid",
        }],
    ] {
        assert!(bundled::install(root, &files).is_err());
        assert_eq!(
            bundled::manage(root, Request::List).unwrap()["active"]["directory"],
            first
        );
    }
    // A half-written directory is not offered as a rollback candidate.
    fs::create_dir_all(root.join("bundled-skills/.stage-interrupted/guide")).unwrap();
    fs::write(
        root.join("bundled-skills/.stage-interrupted/guide/SKILL.md"),
        doc("partial"),
    )
    .unwrap();
    assert_eq!(
        bundled::manage(root, Request::List).unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(bundled::manage(
        root,
        Request::Rollback {
            version: "../escape".into()
        }
    )
    .is_err());
}

#[test]
fn failure_while_staging_preserves_the_previous_complete_release() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = install(root, "first");
    let content = doc("broken new version");
    let result = bundled::install(
        root,
        &[
            BundleFile {
                path: "guide/SKILL.md",
                content: content.as_bytes(),
            },
            BundleFile {
                path: "guide/scripts",
                content: b"a file, not a directory",
            },
            BundleFile {
                path: "guide/scripts/run.sh",
                content: b"cannot be staged",
            },
        ],
    );
    assert!(result.is_err());
    assert_eq!(
        bundled::manage(root, Request::List).unwrap()["active"]["directory"],
        first
    );
    assert_eq!(
        fs::read_to_string(sources(root)[0].join("scripts/run.sh")).unwrap(),
        "echo first\n"
    );
    assert!(!fs::read_dir(root.join("bundled-skills"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".stage-")));
}
