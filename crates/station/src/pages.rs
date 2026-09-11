//! Agent-authored human page delivery and application publication.
use crate::state::AppState;
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    session_id: String,
    action: String,
    request_id: String,
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    description: String,
    id: Option<String>,
}
pub async fn tool(State(state): State<AppState>, Json(request): Json<Request>) -> Response {
    let result = (|| -> Result<Value> {
        let session = state
            .db
            .get_session_by_id(&request.session_id)?
            .context("page_conversation_not_found")?;
        anyhow::ensure!(
            session.platform == crate::im_entry::LOCAL_GUI_PLATFORM,
            "page_requires_local_conversation"
        );
        if request.action == "unpublish" {
            state.db.unpublish_page(
                &session,
                &request.request_id,
                request.id.as_deref().context("missing_page_id")?,
            )?;
            return Ok(json!({"ok":true}));
        }
        let page = crate::db::pages::page_link(
            request.title.as_deref().context("missing_page_title")?,
            request.url.as_deref().context("missing_page_url")?,
            &request.description,
        )?;
        match request.action.as_str() {
            "deliver" => {
                anyhow::ensure!(
                    session.channel_type.as_deref() != Some("agent_control"),
                    "explicit_chat_id_required_use_chat_post_page"
                );
                let message = state
                    .db
                    .deliver_page(&session, &request.request_id, &page)?;
                state.entries.publish_visible_message(&message);
                Ok(json!({"ok":true,"page":page,"message_id":message.message_id}))
            }
            "publish" => Ok(
                json!({"ok":true,"application":state.db.publish_page(&session,&request.request_id,&page)?}),
            ),
            _ => anyhow::bail!("unknown_page_operation"),
        }
    })();
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
