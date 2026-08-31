use std::path::{Path, PathBuf};

const CONTRACT_DOCUMENT: &str = "docs/zork-agent-architecture.md";

#[test]
// Contract: docs/zork-agent-architecture.md [TEST-DOC-01]
fn every_zork_agent_rust_test_references_an_existing_contract() {
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = agent.parent().unwrap().parent().unwrap();
    let document = std::fs::read_to_string(workspace.join(CONTRACT_DOCUMENT))
        .expect("the structured architecture document exists");
    let roots = [
        agent.join("src"),
        agent.join("tests"),
        workspace.join("crates/agent-testkit/src"),
        workspace.join("crates/agent-testkit/tests"),
    ];
    let mut failures = Vec::new();

    for path in roots.into_iter().flat_map(rust_files) {
        let source = std::fs::read_to_string(&path).unwrap();
        let lines = source.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("#[test]")
                && !line.trim_start().starts_with("#[tokio::test")
            {
                continue;
            }
            let start = index.saturating_sub(2);
            let end = (index + 3).min(lines.len());
            let contract = lines[start..end]
                .iter()
                .find_map(|candidate| candidate.split_once("Contract:").map(|(_, value)| value));
            let Some(contract) = contract else {
                failures.push(format!(
                    "{}:{} has no adjacent Contract comment",
                    path.display(),
                    index + 1
                ));
                continue;
            };
            let Some((reference, ids)) = contract.split_once('[') else {
                failures.push(format!(
                    "{}:{} has a malformed Contract comment",
                    path.display(),
                    index + 1
                ));
                continue;
            };
            let Some((ids, _)) = ids.split_once(']') else {
                failures.push(format!(
                    "{}:{} has a malformed Contract comment",
                    path.display(),
                    index + 1
                ));
                continue;
            };
            if reference.trim() != CONTRACT_DOCUMENT {
                failures.push(format!(
                    "{}:{} references contract document {:?} instead of {CONTRACT_DOCUMENT}",
                    path.display(),
                    index + 1,
                    reference.trim()
                ));
            }
            for id in ids.split(',').map(str::trim).filter(|id| !id.is_empty()) {
                if !document.contains(&format!("`{id}`")) {
                    failures.push(format!(
                        "{}:{} references unknown contract {id}",
                        path.display(),
                        index + 1
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "test contract reference failures:\n{}",
        failures.join("\n")
    );
}

fn rust_files(root: PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files);
    files
}

fn collect_rust_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}
