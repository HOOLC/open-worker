//! Read-only core card -> shared visual component, retaining unsubmitted input
//! across metadata updates. No business state is reduced in this adapter.
use crate::{i18n::Locale, transcript::TranscriptLine, views::RootView};
use gpui::{App, AppContext, Entity, WeakEntity};
use std::sync::Arc;
use zork_ui::components::interaction as ui;

pub struct Rendered {
    source: Arc<TranscriptLine>,
    locale: Locale,
    entity: Entity<ui::InteractionCard>,
}

pub fn view(card: &zork_client_core::interactions::Card, locale: Locale) -> ui::View {
    ui::View {
        id: card.message_id.clone(),
        title: if card.localized_title {
            locale.text(&card.title).to_owned()
        } else {
            card.title.clone()
        }
        .into(),
        status: locale.text(&card.status_key).to_owned().into(),
        fields: card
            .fields
            .iter()
            .map(|field| ui::Field {
                id: field.field.id.clone(),
                label: if field.localized_label {
                    locale.text(&field.field.label).to_owned()
                } else {
                    field.field.label.clone()
                }
                .into(),
                kind: match field.field.kind {
                    zork_client_core::interactions::FieldKind::Text => ui::FieldKind::Text,
                    zork_client_core::interactions::FieldKind::Multiline => {
                        ui::FieldKind::Multiline
                    }
                    zork_client_core::interactions::FieldKind::Choice => ui::FieldKind::Choice,
                },
                value: field.value.clone().into(),
                options: field
                    .field
                    .options
                    .iter()
                    .map(|option| (option.value.clone(), option.label.clone().into()))
                    .collect(),
                error: field
                    .error_key
                    .as_ref()
                    .map(|error| locale.text(error).to_owned().into()),
            })
            .collect(),
        details: card
            .details
            .iter()
            .map(|detail| {
                (
                    locale.text(&detail.label_key).to_owned().into(),
                    detail.value.clone().into(),
                )
            })
            .collect(),
        actions: card
            .actions
            .iter()
            .map(|action| ui::Action {
                id: action.id.clone(),
                label: locale.text(&action.label_key).to_owned().into(),
                primary: action.primary,
            })
            .collect(),
        editable: card.editable,
        placeholder: locale.text("interaction_choose").into(),
        error: card.error.clone().map(Into::into),
    }
}

pub fn render(
    source: Arc<TranscriptLine>,
    cache: &super::message::MessageRenderDocument,
    locale: Locale,
    session: &str,
    root: WeakEntity<RootView>,
    cx: &mut App,
) -> Option<Entity<ui::InteractionCard>> {
    let TranscriptLine::Message { metadata, .. } = source.as_ref();
    let card = metadata.interaction_view.as_ref()?;
    let mut cached = cache.interaction.borrow_mut();
    if let Some(rendered) = cached.as_mut() {
        if !Arc::ptr_eq(&rendered.source, &source) || rendered.locale != locale {
            let value = view(card, locale);
            rendered
                .entity
                .update(cx, |card, cx| card.set_view(value, cx));
            rendered.source = source;
            rendered.locale = locale;
        }
        return Some(rendered.entity.clone());
    }
    let value = view(card, locale);
    let session = session.to_owned();
    let id = card.message_id.clone();
    let entity = cx.new(|cx| {
        ui::InteractionCard::new(value, cx).with_action_handler(move |event, cx| {
            let _ = root.update(cx, |view, cx| {
                view.activate_interaction(&session, &id, &event.action, event.values.clone(), cx)
            });
        })
    });
    *cached = Some(Box::new(Rendered {
        source,
        locale,
        entity: entity.clone(),
    }));
    Some(entity)
}
