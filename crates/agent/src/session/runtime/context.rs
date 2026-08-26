use std::{
    io::{self, Write},
    sync::Arc,
};

use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{
    failed_round_placeholder_target, stable_fingerprint, ModelLimits, ToolDefinition,
    READ_CONTEXT_HANDOFF_TOOL_NAME,
};
use crate::session::state::{
    ContextHandoffDocument, ContextHandoffPlan, ContextHandoffState, ProviderMessage,
    SessionSelection, SessionState, ToolCall, TranscriptMessage, TranscriptRole,
};

pub(super) const MODEL_CONTEXT_ESTIMATED_BYTES_PER_TOKEN: u64 = 4;
const MODEL_CONTEXT_BASE_TOKENS: u64 = 256;
pub(super) const MODEL_CONTEXT_MESSAGE_FRAMING_TOKENS: u64 = 64;
const MODEL_CONTEXT_TOOL_FRAMING_TOKENS: u64 = 128;
pub(super) const CONTEXT_HANDOFF_INSTRUCTION: &str = r#"The runtime now requires a context handoff. Call context_handoff exactly once and put the complete handoff document in its document argument. Do not call any other tool in this response.

The current authoritative user requirements are source constraints, not observations or inferences. When those requirements are short, preserve them verbatim instead of paraphrasing them to save space. Do not silently narrow or broaden them, and do not carry requirements explicitly superseded by later user input.

Compress the work history, tool evidence, observations, and inferences into what is required to continue the same work correctly:
- the user's long-term goal and current goal;
- the actual current state: completed, in progress, remaining, and real blockers;
- confirmed product and architecture decisions and boundaries that must not be changed silently;
- the most important observations and inferences, clearly distinguishing observed facts from conclusions;
- next actions and user-observable acceptance conditions.

Do not copy large source passages merely to preserve evidence. For important evidence that may need verification, state its existing natural way to retrieve the original: workspace path and relevant range, the full-output path already returned by a tool, a reproducible command, an external URL, or enough session-history detail to find it with read_session_history. Do not invent citation IDs, reference tables, message indexes, or another schema."#;
pub(super) struct ModelContextMetrics {
    pub(super) visible_input_estimate_tokens: u64,
    pub(super) prompt_fingerprint: String,
    pub(super) tool_schema_fingerprint: String,
}

#[derive(Clone)]
pub(super) struct ContextHandoffPlanDraft {
    pub(super) activation_id: String,
    pub(super) previous_handoff_id: Option<String>,
    pub(super) next_generation: u64,
    pub(super) covered_through_message_id: String,
    pub(super) max_output_tokens: u32,
    pub(super) selection: SessionSelection,
}

impl ContextHandoffPlanDraft {
    pub(super) fn commit(&self, plan_id: String) -> ContextHandoffPlan {
        ContextHandoffPlan {
            plan_id,
            activation_id: self.activation_id.clone(),
            previous_handoff_id: self.previous_handoff_id.clone(),
            next_generation: self.next_generation,
            covered_through_message_id: self.covered_through_message_id.clone(),
            max_output_tokens: self.max_output_tokens,
            selection: self.selection.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct ContextHandoffDocumentDraft {
    pub(super) plan_id: String,
    pub(super) previous_handoff_id: Option<String>,
    pub(super) next_generation: u64,
    pub(super) covered_through_message_id: String,
    pub(super) document: String,
    pub(super) document_tokens: Option<u64>,
    pub(super) selection: SessionSelection,
}

impl ContextHandoffDocumentDraft {
    pub(super) fn commit(&self, handoff_id: String) -> ContextHandoffDocument {
        ContextHandoffDocument {
            handoff_id,
            plan_id: self.plan_id.clone(),
            previous_handoff_id: self.previous_handoff_id.clone(),
            next_generation: self.next_generation,
            covered_through_message_id: self.covered_through_message_id.clone(),
            document: self.document.clone(),
            document_tokens: self.document_tokens,
            selection: self.selection.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct ProviderContextCache {
    cached: Option<CachedProviderContext>,
}

struct CachedProviderContext {
    source_len: usize,
    handoff_id: Option<String>,
    placeholder_target: Option<String>,
    transcript: Arc<Vec<ProviderMessage>>,
}

impl ProviderContextCache {
    pub(super) fn prepare<F>(
        &mut self,
        state: &SessionState,
        system_prompt: &str,
        load_handoff: F,
    ) -> Result<Arc<Vec<ProviderMessage>>, &'static str>
    where
        F: FnOnce(&ContextHandoffState) -> Result<ContextHandoffDocument, &'static str>,
    {
        let handoff_id = state
            .latest_context_handoff
            .as_ref()
            .map(|handoff| handoff.handoff_id.clone());
        let placeholder_target = failed_round_placeholder_target(state);
        let can_extend = self.cached.as_ref().is_some_and(|cached| {
            cached.source_len <= state.transcript.len()
                && cached.handoff_id == handoff_id
                && cached.placeholder_target == placeholder_target
        });

        if can_extend {
            self.extend(state)?;
        } else {
            self.rebuild(
                state,
                handoff_id,
                placeholder_target,
                system_prompt,
                load_handoff,
            )?;
        }
        let cached = self.cached.as_ref().ok_or("model_context_cache")?;
        Ok(cached.transcript.clone())
    }

    fn extend(&mut self, state: &SessionState) -> Result<(), &'static str> {
        let cached = self.cached.as_mut().ok_or("model_context_cache")?;
        let tail = &state.transcript[cached.source_len..];
        let previous_len = cached.transcript.len();
        append_provider_transcript_messages(
            tail,
            cached.placeholder_target.as_deref(),
            Arc::make_mut(&mut cached.transcript),
        );
        debug_assert!(cached.transcript.len() >= previous_len);
        cached.source_len = state.transcript.len();
        Ok(())
    }

    fn rebuild<F>(
        &mut self,
        state: &SessionState,
        handoff_id: Option<String>,
        placeholder_target: Option<String>,
        system_prompt: &str,
        load_handoff: F,
    ) -> Result<(), &'static str>
    where
        F: FnOnce(&ContextHandoffState) -> Result<ContextHandoffDocument, &'static str>,
    {
        let handoff = state
            .latest_context_handoff
            .as_ref()
            .map(load_handoff)
            .transpose()?;
        let transcript = provider_context(state, system_prompt, handoff.as_ref())?;
        self.cached = Some(CachedProviderContext {
            source_len: state.transcript.len(),
            handoff_id,
            placeholder_target,
            transcript: Arc::new(transcript),
        });
        Ok(())
    }
}

pub(super) fn provider_transcript(
    state: &SessionState,
    system_prompt: &str,
    handoff_document: Option<&ContextHandoffDocument>,
) -> Result<Vec<ProviderMessage>, &'static str> {
    let placeholder_target = failed_round_placeholder_target(state);
    let mut projected = Vec::with_capacity(
        state.transcript.len()
            + usize::from(placeholder_target.is_some())
            + usize::from(!system_prompt.is_empty())
            + 2 * usize::from(state.latest_context_handoff.is_some()),
    );
    if !system_prompt.is_empty() {
        projected.push(ProviderMessage {
            role: TranscriptRole::System,
            content: Arc::from(system_prompt),
            is_error: false,
            tool_call_id: None,
            tool_calls: Vec::new(),
            provider_context: None,
        });
    }
    if let Some(handoff) = &state.latest_context_handoff {
        let document = handoff_document
            .filter(|document| handoff.matches_document(document))
            .ok_or("context_handoff_document")?;
        let tool_call_id = format!("call_{}", handoff.handoff_id);
        projected.push(ProviderMessage {
            role: TranscriptRole::Assistant,
            content: Arc::from(""),
            is_error: false,
            tool_call_id: None,
            tool_calls: vec![ToolCall {
                tool_call_id: tool_call_id.clone(),
                tool_name: READ_CONTEXT_HANDOFF_TOOL_NAME.to_owned(),
                arguments: json!({}),
            }],
            provider_context: None,
        });
        projected.push(ProviderMessage {
            role: TranscriptRole::Tool,
            content: Arc::from(document.document.as_str()),
            is_error: false,
            tool_call_id: Some(tool_call_id),
            tool_calls: Vec::new(),
            provider_context: None,
        });
    }
    append_provider_transcript_messages(
        &state.transcript,
        placeholder_target.as_deref(),
        &mut projected,
    );
    if state.latest_context_handoff.is_none() && handoff_document.is_some() {
        return Err("context_handoff_document");
    }
    Ok(projected)
}

fn append_provider_transcript_messages(
    source: &[TranscriptMessage],
    placeholder_target: Option<&str>,
    projected: &mut Vec<ProviderMessage>,
) {
    let placeholder = placeholder_target.map(|_| ProviderMessage {
        role: TranscriptRole::Assistant,
        content: Arc::from(""),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    });
    for original in source {
        if placeholder_target.is_some_and(|target| target == original.message_id) {
            if let Some(placeholder) = &placeholder {
                projected.push(placeholder.clone());
            }
        }
        projected.push(ProviderMessage::from(original));
    }
}

pub(super) fn provider_context(
    state: &SessionState,
    system_prompt: &str,
    handoff_document: Option<&ContextHandoffDocument>,
) -> Result<Vec<ProviderMessage>, &'static str> {
    provider_transcript(state, system_prompt, handoff_document)
}

pub(super) fn build_context_handoff_plan(
    state: &SessionState,
    selection: &SessionSelection,
    max_output_tokens: u32,
) -> Result<Option<ContextHandoffPlanDraft>, &'static str> {
    let Some(boundary) = state.transcript.last() else {
        return Ok(None);
    };
    let activation_id = state
        .active_activation
        .as_ref()
        .map(|activation| activation.activation_id.clone())
        .ok_or("context_handoff_activation_missing")?;
    Ok(Some(ContextHandoffPlanDraft {
        activation_id,
        previous_handoff_id: state
            .latest_context_handoff
            .as_ref()
            .map(|handoff| handoff.handoff_id.clone()),
        next_generation: state
            .latest_context_handoff
            .as_ref()
            .map_or(2, |handoff| handoff.next_generation.saturating_add(1)),
        covered_through_message_id: boundary.message_id.clone(),
        max_output_tokens,
        selection: selection.clone(),
    }))
}

pub(super) fn context_handoff_source(
    state: &SessionState,
    covered_through_message_id: &str,
    ordinary_transcript: &[ProviderMessage],
) -> Result<Vec<ProviderMessage>, &'static str> {
    if state
        .transcript
        .last()
        .map(|message| message.message_id.as_str())
        != Some(covered_through_message_id)
    {
        return Err("context_handoff_boundary");
    }
    let mut source = ordinary_transcript.to_vec();
    source.push(ProviderMessage {
        role: TranscriptRole::User,
        content: Arc::from(CONTEXT_HANDOFF_INSTRUCTION),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    });
    Ok(source)
}

pub(super) fn model_context_metrics(
    transcript: &[ProviderMessage],
    tools: &[ToolDefinition],
) -> Result<ModelContextMetrics, &'static str> {
    let tool_json = serde_json::to_string(
        &tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|_| "model_context_tools_encode")?;
    let visible_input_estimate_tokens = MODEL_CONTEXT_BASE_TOKENS
        .saturating_add(visible_message_tokens(transcript)?)
        .saturating_add((tool_json.len() as u64).div_ceil(MODEL_CONTEXT_ESTIMATED_BYTES_PER_TOKEN))
        .saturating_add(MODEL_CONTEXT_TOOL_FRAMING_TOKENS.saturating_mul(tools.len() as u64));
    Ok(ModelContextMetrics {
        visible_input_estimate_tokens,
        prompt_fingerprint: stable_json_fingerprint(
            "model-prompt",
            transcript,
            "model_context_transcript_encode",
        )?,
        tool_schema_fingerprint: stable_fingerprint("model-tools", &tool_json),
    })
}

fn stable_json_fingerprint<T: Serialize + ?Sized>(
    kind: &str,
    value: &T,
    encode_error: &'static str,
) -> Result<String, &'static str> {
    let mut counter = ByteCounter::default();
    serde_json::to_writer(&mut counter, value).map_err(|_| encode_error)?;

    let mut digest = Sha256::new();
    digest.update(b"zork:runtime-fingerprint:v1");
    digest.update((kind.len() as u64).to_be_bytes());
    digest.update(kind.as_bytes());
    digest.update(counter.bytes.to_be_bytes());
    serde_json::to_writer(DigestWriter(&mut digest), value).map_err(|_| encode_error)?;
    Ok(format!("sha256:v1:{:x}", digest.finalize()))
}

#[derive(Default)]
struct ByteCounter {
    bytes: u64,
}

impl Write for ByteCounter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len() as u64);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn visible_message_tokens(messages: &[ProviderMessage]) -> Result<u64, &'static str> {
    messages.iter().try_fold(0_u64, |total, message| {
        let visible = json!({
            "role": message.role,
            "content": message.content,
            "is_error": message.is_error,
            "tool_call_id": message.tool_call_id,
            "tool_calls": message.tool_calls,
        });
        let mut counter = ByteCounter::default();
        serde_json::to_writer(&mut counter, &visible)
            .map_err(|_| "model_context_visible_message_encode")?;
        let bytes = counter.bytes;
        Ok(total
            .saturating_add(MODEL_CONTEXT_MESSAGE_FRAMING_TOKENS)
            .saturating_add(bytes.div_ceil(MODEL_CONTEXT_ESTIMATED_BYTES_PER_TOKEN)))
    })
}

pub(super) fn model_input_budget(
    limits: &ModelLimits,
    requested_output_tokens: u32,
) -> Option<u64> {
    limits
        .context_window_tokens
        .checked_sub(u64::from(requested_output_tokens))
        .filter(|budget| *budget > 0)
}

pub(super) fn model_context_generation(state: &SessionState) -> u64 {
    state
        .latest_context_handoff
        .as_ref()
        .map_or(1, |handoff| handoff.next_generation)
}

pub(super) fn model_selection_fingerprint(
    selection: &SessionSelection,
) -> Result<String, &'static str> {
    Ok(stable_fingerprint(
        "model-selection",
        &serde_json::to_string(selection).map_err(|_| "model_selection_fingerprint")?,
    ))
}

pub(super) fn estimated_model_input_tokens_from_metrics(
    state: &SessionState,
    transcript: &[ProviderMessage],
    selection_fingerprint: &str,
    tool_schema_fingerprint: &str,
    full_estimate: u64,
    provider_only_tail: &[ProviderMessage],
) -> Result<u64, &'static str> {
    let full_estimate = input_estimate_with_handoff_text_usage(state, transcript, full_estimate)?;
    let Some(anchor) = state.latest_model_usage.as_ref() else {
        return Ok(full_estimate);
    };
    if anchor.selection_fingerprint != selection_fingerprint {
        return Ok(full_estimate);
    }
    if anchor.context_generation != model_context_generation(state)
        || anchor.tool_schema_fingerprint != tool_schema_fingerprint
    {
        return Ok(full_estimate);
    }
    let Some(result_event_id) = anchor.result_event_id.as_deref() else {
        return Ok(full_estimate);
    };
    let Some(result_index) = state
        .transcript
        .iter()
        .position(|message| message.message_id == result_event_id)
    else {
        return Ok(full_estimate);
    };
    let mut projected_tail = Vec::new();
    append_provider_transcript_messages(
        &state.transcript[result_index + 1..],
        failed_round_placeholder_target(state).as_deref(),
        &mut projected_tail,
    );
    let local_tail = visible_message_tokens(&projected_tail)?
        .saturating_add(visible_message_tokens(provider_only_tail)?);
    Ok(anchor
        .input_tokens
        .saturating_add(anchor.output_tokens)
        .saturating_add(local_tail))
}

fn input_estimate_with_handoff_text_usage(
    state: &SessionState,
    transcript: &[ProviderMessage],
    full_estimate: u64,
) -> Result<u64, &'static str> {
    let Some(handoff) = state.latest_context_handoff.as_ref() else {
        return Ok(full_estimate);
    };
    let Some(document_tokens) = handoff.document_tokens else {
        return Ok(full_estimate);
    };
    let tool_call_id = format!("call_{}", handoff.handoff_id);
    let result = transcript
        .iter()
        .find(|message| {
            message.role == TranscriptRole::Tool
                && message.tool_call_id.as_deref() == Some(tool_call_id.as_str())
        })
        .ok_or("context_handoff_projection")?;
    let estimated_with_document = visible_message_tokens(std::slice::from_ref(result))?;
    let mut without_document = result.clone();
    without_document.content = Arc::from("");
    let estimated_without_document =
        visible_message_tokens(std::slice::from_ref(&without_document))?;
    let estimated_document_tokens =
        estimated_with_document.saturating_sub(estimated_without_document);
    Ok(full_estimate
        .saturating_sub(estimated_document_tokens)
        .saturating_add(document_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::{
        ActiveActivation, ContextHandoffState, ModelUsageAnchor, ProviderContext, ToolCall,
    };

    fn selection() -> SessionSelection {
        SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "max".to_owned(),
        }
    }

    fn message(
        message_id: &str,
        role: TranscriptRole,
        content: impl Into<String>,
    ) -> TranscriptMessage {
        TranscriptMessage {
            message_id: message_id.to_owned(),
            role,
            content: Arc::from(content.into()),
            is_error: false,
            tool_call_id: None,
            tool_calls: Vec::new(),
            provider_context: None,
            source_mailbox_seq: None,
        }
    }

    fn active_state(transcript: Vec<TranscriptMessage>) -> SessionState {
        let selection = selection();
        let mut state = SessionState::new("session");
        state.selection = selection.clone();
        state.transcript = transcript;
        state.active_activation = Some(ActiveActivation {
            activation_id: "activation".to_owned(),
            selection,
            started_at_ms: 1,
        });
        state
    }

    fn handoff_state() -> ContextHandoffState {
        ContextHandoffState {
            handoff_id: "01K00000000000000000000000".to_owned(),
            plan_id: "plan".to_owned(),
            previous_handoff_id: None,
            next_generation: 2,
            covered_through_message_id: "covered".to_owned(),
            document_tokens: Some(17),
            selection: selection(),
        }
    }

    fn handoff_document(document: impl Into<String>) -> ContextHandoffDocument {
        ContextHandoffDocument {
            handoff_id: "01K00000000000000000000000".to_owned(),
            plan_id: "plan".to_owned(),
            previous_handoff_id: None,
            next_generation: 2,
            covered_through_message_id: "covered".to_owned(),
            document: document.into(),
            document_tokens: Some(17),
            selection: selection(),
        }
    }

    #[test]
    fn provider_projection_shares_large_message_content_with_the_live_state() {
        let source = message(
            "tool-result",
            TranscriptRole::Tool,
            "large tool output\n".repeat(256 * 1024),
        );

        let projected = ProviderMessage::from(&source);

        assert_eq!(projected.content.len(), source.content.len());
        assert_eq!(projected.content.as_ptr(), source.content.as_ptr());
    }

    #[test]
    fn streamed_prompt_fingerprint_preserves_the_durable_format() {
        let transcript = vec![ProviderMessage::from(&message(
            "message",
            TranscriptRole::User,
            "hello",
        ))];
        let encoded = serde_json::to_string(&transcript).unwrap();

        assert_eq!(
            stable_json_fingerprint("model-prompt", &transcript, "encode").unwrap(),
            stable_fingerprint("model-prompt", &encoded),
        );
    }

    #[test]
    fn encrypted_reasoning_transport_size_does_not_change_visible_token_estimate() {
        let transcript = |encrypted_content: String| {
            let mut assistant = message("assistant", TranscriptRole::Assistant, "visible");
            assistant.provider_context = Some(ProviderContext {
                profile_id: "profile".to_owned(),
                provider: "openai".to_owned(),
                model: "model".to_owned(),
                api: "responses".to_owned(),
                output_items: Arc::new(vec![serde_json::json!({
                    "id": "reasoning",
                    "type": "reasoning",
                    "encrypted_content": encrypted_content,
                })]),
            });
            vec![ProviderMessage::from(&assistant)]
        };

        let small = model_context_metrics(&transcript("x".to_owned()), &[]).unwrap();
        let large = model_context_metrics(&transcript("x".repeat(3 * 1024 * 1024)), &[]).unwrap();

        assert_eq!(
            small.visible_input_estimate_tokens,
            large.visible_input_estimate_tokens
        );
        assert_ne!(small.prompt_fingerprint, large.prompt_fingerprint);
    }

    #[test]
    fn handoff_document_uses_provider_text_tokens_instead_of_estimating_generated_text() {
        let estimate = |document: String| {
            let mut state = active_state(Vec::new());
            state.latest_context_handoff = Some(handoff_state());
            let document = handoff_document(document);
            let transcript = provider_transcript(&state, "system", Some(&document)).unwrap();
            let metrics = model_context_metrics(&transcript, &[]).unwrap();
            estimated_model_input_tokens_from_metrics(
                &state,
                &transcript,
                &model_selection_fingerprint(&selection()).unwrap(),
                &metrics.tool_schema_fingerprint,
                metrics.visible_input_estimate_tokens,
                &[],
            )
            .unwrap()
        };

        assert_eq!(
            estimate("small".to_owned()),
            estimate("x".repeat(3 * 1024 * 1024))
        );
    }

    #[test]
    fn latest_handoff_is_projected_as_a_stable_tool_call_and_result() {
        let mut state = active_state(vec![message("user", TranscriptRole::User, "continue")]);
        state.latest_context_handoff = Some(handoff_state());
        let document = handoff_document("durable handoff");

        let projected = provider_transcript(&state, "system", Some(&document)).unwrap();

        assert_eq!(
            projected
                .iter()
                .map(|message| message.role.clone())
                .collect::<Vec<_>>(),
            vec![
                TranscriptRole::System,
                TranscriptRole::Assistant,
                TranscriptRole::Tool,
                TranscriptRole::User,
            ]
        );
        let call = projected[1]
            .tool_calls
            .first()
            .expect("synthetic tool call");
        assert_eq!(call.tool_name, READ_CONTEXT_HANDOFF_TOOL_NAME);
        assert_eq!(call.arguments, json!({}));
        assert_eq!(call.tool_call_id, "call_01K00000000000000000000000");
        assert!(call.tool_call_id.len() <= 64);
        assert_eq!(
            projected[2].tool_call_id.as_deref(),
            Some(call.tool_call_id.as_str())
        );
        assert_eq!(projected[2].content.as_ref(), "durable handoff");
        assert_eq!(
            provider_transcript(&state, "system", Some(&document)).unwrap(),
            projected
        );
    }

    #[test]
    fn provider_projection_does_not_invent_message_identity() {
        let mut state = active_state(vec![message("user", TranscriptRole::User, "continue")]);
        state.latest_context_handoff = Some(handoff_state());

        let projected =
            provider_transcript(&state, "system", Some(&handoff_document("durable handoff")))
                .unwrap();
        let value = serde_json::to_value(projected).unwrap();
        assert!(value.as_array().unwrap().iter().all(|message| {
            message.get("message_id").is_none()
                && message.get("dedupe_key").is_none()
                && message.get("source_mailbox_seq").is_none()
        }));
    }

    #[test]
    fn handoff_generation_appends_only_an_instruction_to_the_existing_prefix() {
        let state = active_state(vec![
            message("user", TranscriptRole::User, "inspect"),
            TranscriptMessage {
                message_id: "assistant".to_owned(),
                role: TranscriptRole::Assistant,
                content: Arc::from(""),
                is_error: false,
                tool_call_id: None,
                tool_calls: vec![ToolCall {
                    tool_call_id: "call".to_owned(),
                    tool_name: "read".to_owned(),
                    arguments: json!({"path": "README.md"}),
                }],
                provider_context: None,
                source_mailbox_seq: None,
            },
            TranscriptMessage {
                message_id: "tool".to_owned(),
                role: TranscriptRole::Tool,
                content: Arc::from("contents"),
                is_error: false,
                tool_call_id: Some("call".to_owned()),
                tool_calls: Vec::new(),
                provider_context: None,
                source_mailbox_seq: None,
            },
        ]);
        let plan = ContextHandoffPlan {
            plan_id: "plan".to_owned(),
            activation_id: "activation".to_owned(),
            previous_handoff_id: None,
            next_generation: 2,
            covered_through_message_id: "tool".to_owned(),
            max_output_tokens: 128_000,
            selection: selection(),
        };

        let ordinary = provider_transcript(&state, "system", None).unwrap();
        let handoff =
            context_handoff_source(&state, &plan.covered_through_message_id, &ordinary).unwrap();

        assert_eq!(&handoff[..ordinary.len()], ordinary.as_slice());
        assert_eq!(handoff.len(), ordinary.len() + 1);
        assert_eq!(handoff.last().unwrap().role, TranscriptRole::User);
        assert!(handoff.last().unwrap().content.contains("context_handoff"));
    }

    #[test]
    fn handoff_input_estimate_counts_the_provider_only_instruction_after_a_usage_anchor() {
        let mut state = active_state(vec![
            message("user", TranscriptRole::User, "inspect"),
            message("assistant", TranscriptRole::Assistant, "working"),
        ]);
        let selection_fingerprint = model_selection_fingerprint(&selection()).unwrap();
        let ordinary = provider_transcript(&state, "system", None).unwrap();
        let ordinary_metrics = model_context_metrics(&ordinary, &[]).unwrap();
        state.latest_model_usage = Some(ModelUsageAnchor {
            context_generation: 1,
            selection_fingerprint: selection_fingerprint.clone(),
            tool_schema_fingerprint: ordinary_metrics.tool_schema_fingerprint.clone(),
            result_event_id: Some("assistant".to_owned()),
            input_tokens: 1_000,
            cached_input_tokens: Some(900),
            output_tokens: 100,
            output_reasoning_tokens: Some(80),
            output_text_tokens: Some(20),
        });
        let ordinary_estimate = estimated_model_input_tokens_from_metrics(
            &state,
            &ordinary,
            &selection_fingerprint,
            &ordinary_metrics.tool_schema_fingerprint,
            ordinary_metrics.visible_input_estimate_tokens,
            &[],
        )
        .unwrap();
        let handoff = context_handoff_source(&state, "assistant", &ordinary).unwrap();
        let handoff_metrics = model_context_metrics(&handoff, &[]).unwrap();
        let handoff_estimate = estimated_model_input_tokens_from_metrics(
            &state,
            &handoff,
            &selection_fingerprint,
            &handoff_metrics.tool_schema_fingerprint,
            handoff_metrics.visible_input_estimate_tokens,
            &handoff[ordinary.len()..],
        )
        .unwrap();
        let provider_only_tail = visible_message_tokens(&handoff[ordinary.len()..]).unwrap();

        assert_eq!(ordinary_estimate, 1_100);
        assert_eq!(handoff_estimate, ordinary_estimate + provider_only_tail);
    }

    #[test]
    fn handoff_instruction_preserves_current_authoritative_user_requirements_verbatim() {
        assert!(CONTEXT_HANDOFF_INSTRUCTION.contains("authoritative user requirements"));
        assert!(CONTEXT_HANDOFF_INSTRUCTION.contains("preserve them verbatim"));
        assert!(CONTEXT_HANDOFF_INSTRUCTION.contains("Compress the work history"));
        assert!(CONTEXT_HANDOFF_INSTRUCTION.contains("superseded"));
        assert!(CONTEXT_HANDOFF_INSTRUCTION.contains("context_handoff"));
    }

    #[test]
    fn handoff_plan_never_moves_the_boundary_backward_to_fit() {
        let state = active_state(vec![
            message("first", TranscriptRole::User, "small"),
            message("latest", TranscriptRole::User, "x".repeat(256 * 1024)),
        ]);

        let plan = build_context_handoff_plan(&state, &selection(), 128_000)
            .unwrap()
            .expect("handoff plan");

        assert_eq!(plan.covered_through_message_id, "latest");
    }
}
