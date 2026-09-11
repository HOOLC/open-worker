//! Content membership is a core projection of explicit owner records.
use crate::api::Artifact;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
pub use zork_client_types::pages::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentIndex {
    File(usize),
    Page(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentKind {
    Page,
    File,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConversationContents {
    pub pages: Arc<Vec<ContentIndex>>,
    pub files: Arc<Vec<ContentIndex>>,
}

pub type ContentCatalog = HashMap<Option<String>, Arc<ConversationContents>>;

impl ConversationContents {
    pub fn entries(&self, kind: ContentKind) -> &Arc<Vec<ContentIndex>> {
        match kind {
            ContentKind::Page => &self.pages,
            ContentKind::File => &self.files,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty() && self.files.is_empty()
    }
}

/// Search within an already scoped conversation group. Empty queries share its index.
pub fn filter_contents(
    entries: &Arc<Vec<ContentIndex>>,
    files: &[Artifact],
    pages: &PageCatalog,
    query: &str,
) -> Arc<Vec<ContentIndex>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return entries.clone();
    }
    let matches = |text: &str| text.to_lowercase().contains(&query);
    Arc::new(
        entries
            .iter()
            .copied()
            .filter(|entry| match *entry {
                ContentIndex::File(i) => files.get(i).is_some_and(|f| matches(&f.name)),
                ContentIndex::Page(i) => pages.references.get(i).is_some_and(|p| {
                    matches(&p.page.title) || matches(&p.page.description) || matches(&p.page.url)
                }),
            })
            .collect(),
    )
}

pub enum LinkAction {
    Embedded(String),
    External(String),
}
impl crate::state::Device {
    pub fn link_action(&self, session: Option<&str>, url: &str) -> LinkAction {
        let state = self.snapshot();
        let delivered = state
            .pages
            .references
            .iter()
            .any(|p| Some(p.session_id.as_str()) == session && p.page.url == url);
        if delivered || zork_mesh::services::ServiceLink::parse(url).is_ok() {
            LinkAction::Embedded(url.into())
        } else {
            LinkAction::External(url.into())
        }
    }
}

pub fn content_indices(files: &[Artifact], pages: &PageCatalog) -> ContentCatalog {
    let mut groups: HashMap<Option<String>, Vec<ContentIndex>> = HashMap::new();
    let ids: HashMap<_, _> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.artifact_id.as_str(), i))
        .collect();
    for (i, file) in files.iter().enumerate() {
        groups
            .entry(file.session_id.clone())
            .or_default()
            .push(ContentIndex::File(i));
    }
    for reference in &pages.files {
        if let Some(&i) = ids.get(reference.artifact_id.as_str()) {
            groups
                .entry(Some(reference.session_id.clone()))
                .or_default()
                .push(ContentIndex::File(i));
        }
    }
    for (i, reference) in pages.references.iter().enumerate() {
        groups
            .entry(Some(reference.session_id.clone()))
            .or_default()
            .push(ContentIndex::Page(i));
    }
    groups
        .into_iter()
        .map(|(session, mut entries)| {
            let mut seen = HashSet::new();
            entries.retain(|entry| {
                seen.insert(match entry {
                    ContentIndex::File(i) => format!("file:{}", files[*i].artifact_id),
                    ContentIndex::Page(i) => pages.references[*i].page.id.clone(),
                })
            });
            let key = |entry: &ContentIndex| match entry {
                ContentIndex::File(i) => (
                    files[*i].created_at.as_str(),
                    1,
                    files[*i].artifact_id.as_str(),
                ),
                ContentIndex::Page(i) => (
                    pages.references[*i].created_at.as_str(),
                    0,
                    pages.references[*i].page.id.as_str(),
                ),
            };
            entries.sort_by(|a, b| {
                let a = key(a);
                let b = key(b);
                b.0.cmp(a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2))
            });
            let (files, pages): (Vec<_>, Vec<_>) = entries
                .into_iter()
                .partition(|entry| matches!(entry, ContentIndex::File(_)));
            (
                session,
                Arc::new(ConversationContents {
                    files: Arc::new(files),
                    pages: Arc::new(pages),
                }),
            )
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationEntry {
    pub page: PageLink,
    pub device_id: String,
    pub device_name: String,
    pub offline: bool,
}

#[derive(Clone, Default)]
pub(crate) struct ApplicationSource {
    pub applications: Arc<Vec<Application>>,
    pub online: Option<bool>,
    pub origin: Option<String>,
}

pub(crate) fn applications(
    nodes: &[(String, String)],
    sources: &HashMap<String, ApplicationSource>,
) -> Vec<ApplicationEntry> {
    let mut by_page = HashMap::<String, ApplicationEntry>::new();
    for (device, name) in nodes {
        let Some(source) = sources.get(device) else {
            continue;
        };
        for app in source.applications.iter() {
            let runtime = zork_mesh::services::ServiceLink::parse(&app.page.url)
                .ok()
                .and_then(|link| {
                    nodes.iter().find(|(id, _)| {
                        sources
                            .get(id)
                            .is_some_and(|s| s.origin.as_deref() == Some(link.origin.as_str()))
                    })
                });
            let offline = runtime
                .is_some_and(|(id, _)| sources.get(id).is_some_and(|s| s.online == Some(false)));
            let (runtime_id, runtime_name) = runtime
                .cloned()
                .unwrap_or_else(|| (device.clone(), name.clone()));
            let entry = ApplicationEntry {
                page: app.page.clone(),
                device_id: runtime_id,
                device_name: runtime_name,
                offline,
            };
            by_page
                .entry(app.page.id.clone())
                .and_modify(|old| {
                    if old.offline && !entry.offline {
                        *old = entry.clone();
                    }
                })
                .or_insert(entry);
        }
    }
    let mut entries = by_page.into_values().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.page
            .title
            .cmp(&b.page.title)
            .then(a.page.id.cmp(&b.page.id))
    });
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(id: &str) -> PageLink {
        PageLink {
            id: id.into(),
            title: "Report".into(),
            url: format!("https://example.test/{id}"),
            description: String::new(),
        }
    }
    #[test]
    fn only_explicit_references_enter_a_conversation() {
        let catalog = PageCatalog {
            references: vec![ConversationPage {
                id: "r".into(),
                session_id: "leader".into(),
                message_id: "m".into(),
                page: page("report"),
                source_session_id: Some("worker".into()),
                created_at: "2".into(),
            }],
            applications: vec![Application {
                page: page("other"),
                owner_session_id: "elsewhere".into(),
                created_at: "1".into(),
            }],
            ..Default::default()
        };
        let index = content_indices(&[], &catalog);
        let contents = &index[&Some("leader".into())];
        assert_eq!(*contents.pages, vec![ContentIndex::Page(0)]);
        assert!(contents.files.is_empty());
        assert_eq!(
            *filter_contents(&contents.pages, &[], &catalog, "REPORT"),
            vec![ContentIndex::Page(0)]
        );
        assert!(filter_contents(&contents.pages, &[], &catalog, "other").is_empty());
        assert!(Arc::ptr_eq(
            &contents.pages,
            &filter_contents(&contents.pages, &[], &catalog, "  ")
        ));
        assert!(!index.contains_key(&Some("worker".into())));
    }

    #[test]
    fn grouped_files_keep_versions_order_and_search_scope() {
        let file = |id: &str, session: &str, created: &str, version: i64| Artifact {
            artifact_id: id.into(),
            task_id: None,
            session_id: Some(session.into()),
            task_title: String::new(),
            workspace: String::new(),
            name: "Report.HTML".into(),
            source_path: String::new(),
            media_type: "text/html".into(),
            caption: None,
            byte_len: 10,
            version,
            created_at: created.into(),
        };
        let files = vec![
            file("v1", "chat", "1", 1),
            file("v2", "chat", "2", 2),
            file("other", "other", "3", 1),
        ];
        let catalog = PageCatalog {
            files: vec![ConversationFile {
                id: "duplicate-reference".into(),
                session_id: "chat".into(),
                artifact_id: "v1".into(),
                source_session_id: "worker".into(),
            }],
            ..Default::default()
        };
        let groups = content_indices(&files, &catalog);
        let contents = &groups[&Some("chat".into())];
        assert_eq!(
            *contents.files,
            vec![ContentIndex::File(1), ContentIndex::File(0)]
        );
        assert!(contents.pages.is_empty());
        assert_eq!(
            *filter_contents(
                contents.entries(ContentKind::File),
                &files,
                &catalog,
                "report.html"
            ),
            *contents.files
        );
        assert!(filter_contents(
            contents.entries(ContentKind::Page),
            &files,
            &catalog,
            "report"
        )
        .is_empty());
        assert!(filter_contents(&contents.files, &files, &catalog, "missing").is_empty());
    }
    #[test]
    fn application_publication_is_global_but_removed_devices_are_excluded() {
        let source = ApplicationSource {
            applications: Arc::new(vec![Application {
                page: page("report"),
                owner_session_id: "task".into(),
                created_at: "1".into(),
            }]),
            online: Some(false),
            origin: None,
        };
        let sources = HashMap::from([("a".into(), source)]);
        let entries = applications(&[("a".into(), "Device".into())], &sources);
        assert_eq!(entries.len(), 1);
        assert!(
            !entries[0].offline,
            "public HTTP page does not depend on its publisher being online"
        );
        assert!(applications(&[], &sources).is_empty());
    }
}
