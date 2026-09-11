//! Development-only component stories. Every sample calls production renderers.
use super::{agents::AgentsView, profiles::ProfilesView, ui};
#[cfg(not(target_family = "wasm"))]
use crate::{api::Role, transcript::TranscriptLine, views::RootView};
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    design::CUE_UI,
};
use gpui::{div, prelude::*, px, rgb, AnyView, Context, Window};
use serde_json::{json, Value};
#[cfg(not(target_family = "wasm"))]
use std::sync::Arc;

pub use zork_ui::stories::{PrimitiveStory, Story};
fn click(id: &str) -> Value {
    json!({"type":"click","target":{"element_id":id}})
}
pub fn catalog() -> Vec<Story> {
    let mut items = zork_ui::stories::catalog();
    for (family, title, state, target, actions, reference) in [
        (
            "connection",
            "大模型",
            "list",
            "desktop-settings-column",
            vec![],
            "connection-list",
        ),
        (
            "connection",
            "大模型",
            "create",
            "profile-create-dialog",
            vec![click("profile-add")],
            "connection-create",
        ),
        (
            "connection",
            "大模型",
            "provider",
            "profile-create-dialog",
            vec![click("profile-add"), click("profile-provider-select")],
            "connection-provider",
        ),
        (
            "model",
            "模型配置",
            "detail",
            "profile-detail-dialog",
            vec![],
            "model-detail",
        ),
        (
            "model",
            "模型配置",
            "create",
            "model-editor-dialog",
            vec![
                click("profile-model-add"),
                click("profile-context-limit"),
                json!({"type":"type_text","text":"32000"}),
                click("profile-output-limit"),
                json!({"type":"type_text","text":"4096"}),
                click("profile-model"),
            ],
            "model-create",
        ),
        (
            "model",
            "模型配置",
            "protocol",
            "model-editor-dialog",
            vec![click("profile-model-add"), click("model-api-select")],
            "model-protocol",
        ),
        (
            "agent",
            "队员",
            "list",
            "desktop-settings-column",
            vec![],
            "agent-list",
        ),
        (
            "agent",
            "队员",
            "create",
            "agent-create-dialog",
            vec![click("agent-add")],
            "agent-create",
        ),
        (
            "agent",
            "队员",
            "edit",
            "agent-editor-dialog",
            vec![click("agent-settings-leader")],
            "agent-edit",
        ),
        (
            "agent",
            "队员",
            "dropdown",
            "agent-editor-dialog",
            vec![
                click("agent-settings-leader"),
                click("agent-edit-profile-select"),
            ],
            "agent-dropdown",
        ),
        (
            "conversation",
            "会话",
            "messages",
            "story-component",
            vec![],
            "conversation",
        ),
        (
            "conversation",
            "会话",
            "composer",
            "composer-surface",
            vec![],
            "composer",
        ),
        (
            "conversation",
            "会话",
            "history",
            "story-component",
            vec![],
            "conversation-history",
        ),
    ] {
        for (width, height, suffix) in [(900., 600., "compact"), (1280., 800., "wide")] {
            let mut story = Story::new(
                family,
                title,
                &format!("{state}-{suffix}"),
                if family == "conversation" {
                    "views.rs (production desktop fixture)"
                } else if family == "agent" {
                    "desktop/agents.rs"
                } else {
                    "desktop/profiles.rs"
                },
                reference,
            );
            story.width = width;
            story.height = height;
            story.target = target.into();
            story.actions = actions.clone();
            items.push(story);
        }
    }
    for (family, title, states) in [
        (
            "client",
            "客户端设置",
            &["signed-out", "signed-in", "loading", "error"][..],
        ),
        (
            "device",
            "设备设置",
            &["running", "stopped", "loading", "error"][..],
        ),
        ("mesh", "设备连接", &["connected", "empty", "manual"][..]),
        (
            "enrollment",
            "连接设备",
            &["start", "command", "loading", "error", "expired"][..],
        ),
    ] {
        for state in states {
            for (width, height, suffix) in [(900., 600., "compact"), (1280., 800., "wide")] {
                let mut story = Story::new(
                    family,
                    title,
                    &format!("{state}-{suffix}"),
                    if matches!(family, "mesh" | "enrollment") {
                        "zork-ui/src/network.rs"
                    } else {
                        "zork-ui/src/settings.rs"
                    },
                    family,
                );
                story.width = width;
                story.height = height;
                story.target = if family == "enrollment" {
                    "add-device-dialog"
                } else if family == "mesh" && *state == "manual" {
                    "mesh-peer-dialog"
                } else {
                    "desktop-settings-column"
                }
                .into();
                items.push(story);
            }
        }
    }
    let form = zork_ui::stories::page_fixture()["model_form"].clone();
    for story in &mut items {
        if story.family == "model"
            && (story.state.starts_with("create") || story.state.starts_with("protocol"))
        {
            story.actions = vec![
                click("profile-model-add"),
                click("profile-context-limit"),
                json!({"type":"type_text","text":form["context_window"].as_u64().unwrap().to_string()}),
                click("profile-output-limit"),
                json!({"type":"type_text","text":form["max_output_tokens"].as_u64().unwrap().to_string()}),
                click(if story.state.starts_with("protocol") {
                    "model-api-select"
                } else {
                    "profile-model"
                }),
            ];
        }
    }
    items
}

pub struct StoryHost {
    inner: AnyView,
    settings: bool,
    #[cfg(not(target_family = "wasm"))]
    _directory: tempfile::TempDir,
}
impl StoryHost {
    pub fn inspect(&self, cx: &gpui::App) -> Value {
        if let Ok(view) = self.inner.clone().downcast::<PrimitiveStory>() {
            return view.read(cx).inspect(cx);
        }
        if let Ok(view) = self.inner.clone().downcast::<ProfilesView>() {
            return view.read(cx).headless_state(cx);
        }
        json!({})
    }

    pub fn new(story: Story, cx: &mut Context<Self>) -> Self {
        #[cfg(not(target_family = "wasm"))]
        let directory = tempfile::tempdir().expect("isolated story directory");
        let settings = matches!(
            story.family.as_str(),
            "connection" | "model" | "agent" | "client" | "device" | "mesh" | "enrollment"
        );
        let inner = match story.family.as_str() {
            "mesh" | "enrollment" => cx
                .new(|cx| {
                    zork_ui::network::NetworkStory::new(
                        story.family.clone(),
                        story
                            .state
                            .trim_end_matches("-compact")
                            .trim_end_matches("-wide")
                            .into(),
                        cx,
                    )
                })
                .into(),
            "client" | "device" => cx
                .new(|cx| {
                    zork_ui::settings::SettingsStory::new(
                        story.family.clone(),
                        story
                            .state
                            .trim_end_matches("-compact")
                            .trim_end_matches("-wide")
                            .into(),
                        cx,
                    )
                })
                .into(),
            "connection" => cx
                .new(|cx| ProfilesView::headless_fixture(false, cx))
                .into(),
            "model" => cx.new(|cx| ProfilesView::headless_fixture(true, cx)).into(),
            "agent" => cx.new(AgentsView::headless_fixture).into(),
            #[cfg(not(target_family = "wasm"))]
            "conversation" => {
                let store = Arc::new(
                    super::store::ClientStore::open(directory.path()).expect("story store"),
                );
                cx.new(|cx| {
                    let history = story.state.starts_with("history");
                    let mut view = RootView::render_benchmark_fixture(history, store, cx);
                    {
                        let fixture = zork_ui::stories::page_fixture();
                        let messages = fixture["conversation"]["messages"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|message| TranscriptLine::Message {
                                role: if message["role"] == "user" {
                                    Role::User
                                } else {
                                    Role::Assistant
                                },
                                content: message["content"].as_str().unwrap().into(),
                                metadata: crate::api::MessageMetadata {
                                    id: message["id"].as_str().map(str::to_owned),
                                    created_at: message["created_at"].as_str().map(str::to_owned),
                                    author_agent_id: if message["role"] == "assistant" {
                                        Some("leader".into())
                                    } else {
                                        None
                                    },
                                    author_name: if message["role"] == "assistant" {
                                        Some("产品领队".into())
                                    } else {
                                        None
                                    },
                                    author_avatar: if message["role"] == "assistant" {
                                        Some("fox".into())
                                    } else {
                                        None
                                    },
                                    device: Some("mini1".into()),
                                    ..Default::default()
                                },
                            })
                            .collect();
                        view.benchmark_replace_messages(messages, cx);
                        view.benchmark_story_placeholder(
                            fixture["conversation"]["placeholder"]
                                .as_str()
                                .unwrap()
                                .to_owned(),
                            cx,
                        );
                        if history {
                            view.benchmark_story_history(
                                serde_json::from_value(fixture["history"]["records"].clone())
                                    .unwrap(),
                                fixture["history"]["now"].as_i64().unwrap(),
                                cx,
                            );
                        }
                    }
                    view
                })
                .into()
            }
            _ => cx.new(|cx| PrimitiveStory::new(story, cx)).into(),
        };
        Self {
            inner,
            settings,
            #[cfg(not(target_family = "wasm"))]
            _directory: directory,
        }
    }
}
impl Render for StoryHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let inner = if self.settings {
            div().size_full().bg(rgb(CUE_UI.palette.sidebar)).child(
                div()
                    .ml(px(240.))
                    .h_full()
                    .bg(rgb(CUE_UI.palette.canvas))
                    .child(ui::settings_content(self.inner.clone())),
            )
        } else {
            div().size_full().child(self.inner.clone())
        };
        div()
            .id("story-component")
            .size_full()
            .font_family("Inter Variable")
            .text_size(px(13.))
            .text_color(rgb(CUE_UI.palette.text))
            .bg(rgb(CUE_UI.palette.canvas))
            .child(inner)
            .automation(AutomationRole::Status, "组件画布")
    }
}
