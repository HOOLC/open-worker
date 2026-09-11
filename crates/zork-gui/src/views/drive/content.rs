//! Conversation content tabs. Core owns membership, ordering and search results.
use super::*;
use crate::components::text_input::{ComposerEdited, ComposerInput};
use zork_client_core::pages::{ContentIndex, ContentKind};

pub(super) struct ContentTabState {
    input: Entity<ComposerInput>,
    query: String,
    source: Option<Arc<Vec<ContentIndex>>>,
    rows: Arc<Vec<ContentIndex>>,
    scroll: gpui::UniformListScrollHandle,
}

pub(in crate::views) fn tab_id(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Page => "conversation-content-pages",
        ContentKind::File => "conversation-content-files",
    }
}

pub(super) fn title_key(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Page => "content_pages",
        ContentKind::File => "content_files",
    }
}

pub(super) fn all_id(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Page => "conversation-pages-all",
        ContentKind::File => "conversation-files-all",
    }
}

fn search_id(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Page => "content-search-pages",
        ContentKind::File => "content-search-files",
    }
}

fn icon(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Page => "browser/globe.svg",
        ContentKind::File => "icons/file.svg",
    }
}

impl RootView {
    pub(in crate::views) fn active_content_kind(&self, cx: &gpui::App) -> Option<ContentKind> {
        [ContentKind::Page, ContentKind::File]
            .into_iter()
            .find(|kind| self.browser.read(cx).is_native_page_active(tab_id(*kind)))
    }

    pub(in crate::views) fn close_content_tab(&mut self, id: &str) {
        if let Some(kind) = [ContentKind::Page, ContentKind::File]
            .into_iter()
            .find(|kind| tab_id(*kind) == id)
        {
            self.drive.tabs.remove(&(self.browser_host(), kind));
            self.regions.retain(|key| key != tab_id(kind));
        }
    }

    fn ensure_content_tab(&mut self, kind: ContentKind, cx: &mut Context<Self>) {
        let key = (self.browser_host(), kind);
        if self.drive.tabs.contains_key(&key) {
            return;
        }
        let input =
            cx.new(|cx| ComposerInput::new(self.locale.text("content_search"), cx).single_line());
        let search_key = key.clone();
        cx.subscribe(&input, move |view, input, _: &ComposerEdited, cx| {
            if let Some(tab) = view.drive.tabs.get_mut(&search_key) {
                tab.query = input.read(cx).value().to_owned();
                tab.source = None;
                tab.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
            }
            zork_ui::components::region::invalidate(cx, &[tab_id(search_key.1)]);
        })
        .detach();
        self.drive.tabs.insert(
            key,
            ContentTabState {
                input,
                query: String::new(),
                source: None,
                rows: Default::default(),
                scroll: Default::default(),
            },
        );
    }

    pub(super) fn open_content_tab(&mut self, kind: ContentKind, cx: &mut Context<Self>) {
        if self.selected_session.is_none() {
            return;
        }
        self.ensure_content_tab(kind, cx);
        self.close_conversation_files();
        let host = self.browser_host();
        let title = self.locale.text(title_key(kind)).to_owned();
        self.browser.update(cx, |browser, cx| {
            browser.set_host(host, cx);
            browser.open_native_page(
                crate::browser::NativePage {
                    id: tab_id(kind).into(),
                    title,
                    icon: icon(kind),
                },
                cx,
            );
        });
        zork_ui::components::region::invalidate_all(cx);
    }

    pub(in crate::views) fn render_content_page(
        &mut self,
        kind: ContentKind,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_content_tab(kind, cx);
        let entries = self.conversation_contents().entries(kind).clone();
        let key = (self.browser_host(), kind);
        let tab = self.drive.tabs.get_mut(&key).expect("content tab");
        if tab
            .source
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &entries))
        {
            tab.rows = zork_client_core::pages::filter_contents(
                &entries,
                &self.drive.items,
                &self.drive.pages,
                &tab.query,
            );
            tab.source = Some(entries);
        }
        tab.input.update(cx, |input, cx| {
            input.set_placeholder(self.locale.text("content_search"), cx)
        });
        let rows = tab.rows.clone();
        let count = rows.len();
        let scroll = tab.scroll.clone();
        let input = tab.input.clone();
        let query_empty = tab.query.trim().is_empty();
        let title = self.locale.text(title_key(kind));
        div()
            .id(tab_id(kind))
            .size_full()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(CUE_UI.palette.canvas))
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(crate::desktop::ui::text_role(
                        format!("{title} · {count}"),
                        zork_ui::design::TextRole::SectionTitle,
                    ))
                    .child(
                        crate::desktop::ui::input_control(search_id(kind), &input, false, cx)
                            .automation(
                                AutomationRole::TextInput,
                                self.locale.text("content_search"),
                            ),
                    ),
            )
            .child(if count == 0 {
                div()
                    .px_4()
                    .py_6()
                    .child(crate::desktop::ui::text_role(
                        self.locale.text(if query_empty {
                            match kind {
                                ContentKind::Page => "content_pages_empty",
                                ContentKind::File => "content_files_empty",
                            }
                        } else {
                            "content_search_empty"
                        }),
                        zork_ui::design::TextRole::Description,
                    ))
                    .into_any_element()
            } else {
                gpui::uniform_list(
                    match kind {
                        ContentKind::Page => "conversation-pages-list",
                        ContentKind::File => "conversation-artifacts",
                    },
                    count,
                    cx.processor(move |view, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| {
                                div()
                                    .h(px(52.))
                                    .px_2()
                                    .pb_1()
                                    .child(view.render_content_row(rows[row], true, cx))
                            })
                            .collect()
                    }),
                )
                .track_scroll(&scroll)
                .flex_1()
                .min_h_0()
                .w_full()
                .automation(AutomationRole::ScrollArea, title)
                .into_any_element()
            })
            .automation(AutomationRole::Status, title)
    }

    pub(super) fn render_content_row(
        &self,
        index: ContentIndex,
        full: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        #[cfg(feature = "headless-bench")]
        {
            let key = match index {
                ContentIndex::File(i) => i,
                ContentIndex::Page(i) => self.drive.items.len() + i,
            };
            let mut visible = self.benchmark_artifact_indices.borrow_mut();
            visible.insert(key);
            self.benchmark_artifact_cards.set(visible.len());
        }
        match index {
            ContentIndex::File(i) => {
                let artifact = &self.drive.items[i];
                let open = artifact.clone();
                let meta = if artifact.version > 1 {
                    format!("{} · v{}", file_size(artifact.byte_len), artifact.version)
                } else {
                    file_size(artifact.byte_len)
                };
                let prefix = if full {
                    "content-file"
                } else {
                    "conversation-artifact"
                };
                zork_ui::components::attachments::content_row(
                    format!("{prefix}-{}", artifact.artifact_id),
                    "icons/file.svg",
                    artifact.name.clone(),
                    meta,
                    cx,
                    move |v, cx| v.select_artifact(open.clone(), cx),
                )
                .into_any_element()
            }
            ContentIndex::Page(i) => {
                let reference = &self.drive.pages.references[i];
                let page = reference.page.clone();
                let meta = if reference.source_session_id.is_some() {
                    self.locale.text("content_handed_page").to_owned()
                } else {
                    reference.page.description.clone()
                };
                let prefix = if full {
                    "content-page"
                } else {
                    "conversation-page"
                };
                zork_ui::components::attachments::content_row(
                    format!("{prefix}-{}", reference.id),
                    "browser/globe.svg",
                    page.title.clone(),
                    meta,
                    cx,
                    move |v, cx| v.open_page(page.clone(), cx),
                )
                .into_any_element()
            }
        }
    }
}
