use super::*;

pub(super) async fn drain_mailbox(
    store: Arc<dyn EventStore>,
    session_id: String,
    state: VerifiedSessionState,
) -> Result<(Option<AppendResult>, VerifiedSessionState), &'static str> {
    tokio::task::spawn_blocking(move || drain_mailbox_blocking(&*store, &session_id, state))
        .await
        .map_err(|_| "mailbox_drain_join")?
}

fn drain_mailbox_blocking(
    store: &dyn EventStore,
    session_id: &str,
    mut state: VerifiedSessionState,
) -> Result<(Option<AppendResult>, VerifiedSessionState), &'static str> {
    for _ in 0..16 {
        let Some(through_mailbox_seq) = state.mailbox.last().map(|message| message.mailbox_seq)
        else {
            return Ok((None, state));
        };
        let events = EventDraft::single(SessionEvent::MailboxDrained {
            through_mailbox_seq,
        });
        match store.append_verified(session_id, state, &events) {
            Ok(appended) => return Ok((Some(appended.append), appended.state)),
            Err(StoreError::OptimisticConcurrency { .. }) => {
                state = store
                    .rehydrate_verified(session_id)
                    .map_err(|_| "mailbox_drain_rehydrate")?;
            }
            Err(_) => return Err("mailbox_drain_append"),
        }
    }
    Err("mailbox_drain_concurrency")
}

pub(super) struct ModelResultInput {
    pub(super) identity: PreparedRequestIdentity,
    pub(super) attempt_id: String,
    pub(super) usage: Option<ModelUsageAnchor>,
    pub(super) assistant_content: String,
    pub(super) provider_context: Option<ProviderContext>,
    pub(super) tool_calls: Vec<ToolCall>,
    pub(super) provider_input: Option<ProviderInputDiagnostics>,
}

pub(super) async fn append_model_result(
    store: Arc<dyn EventStore>,
    session_id: String,
    state: VerifiedSessionState,
    input: ModelResultInput,
) -> Result<(AppendResult, VerifiedSessionState), String> {
    tokio::task::spawn_blocking(move || {
        append_model_result_blocking(&*store, &session_id, state, &input)
    })
    .await
    .map_err(|error| format!("model_result_join: {error}"))?
}

fn append_model_result_blocking(
    store: &dyn EventStore,
    session_id: &str,
    mut state: VerifiedSessionState,
    input: &ModelResultInput,
) -> Result<(AppendResult, VerifiedSessionState), String> {
    for _ in 0..16 {
        let events = EventDraft::batch(2, |event_ids| {
            let result_message_id = event_ids[1].clone();
            let usage = input.usage.clone().map(|mut usage| {
                usage.result_event_id = Some(result_message_id.clone());
                usage
            });
            vec![
                SessionEvent::ModelRequestCompleted {
                    activation_id: input.identity.activation_id.clone(),
                    round_id: input.identity.round_id.clone(),
                    request_id: input.identity.request_id.clone(),
                    attempt_id: input.attempt_id.clone(),
                    usage,
                    provider_input: input.provider_input.clone(),
                },
                SessionEvent::MessageAppended {
                    message: TranscriptMessage {
                        message_id: result_message_id,
                        role: TranscriptRole::Assistant,
                        content: input.assistant_content.clone().into(),
                        is_error: false,
                        tool_call_id: None,
                        tool_calls: input.tool_calls.clone(),
                        provider_context: input.provider_context.clone(),
                        source_mailbox_seq: None,
                    },
                    wake_wait: false,
                },
            ]
        });
        match store.append_verified(session_id, state, &events) {
            Ok(appended) => return Ok((appended.append, appended.state)),
            Err(StoreError::OptimisticConcurrency { .. }) => {
                state = store
                    .rehydrate_verified(session_id)
                    .map_err(|error| format!("model_result_rehydrate: {error}"))?;
            }
            Err(error) => return Err(format!("model_result_append: {error}")),
        }
    }
    Err("model_result_concurrency".to_owned())
}

pub(super) async fn append_tool_result(
    store: Arc<dyn EventStore>,
    session_id: String,
    state: VerifiedSessionState,
    call: ToolCall,
    result: ToolExecutionResult,
    control: ToolResultControl,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    tokio::task::spawn_blocking(move || {
        append_tool_result_blocking(&*store, &session_id, state, &call, &result, &control)
    })
    .await
    .map_err(|_| "tool_result_join")?
}

pub(super) async fn append_interrupted_tool_results(
    store: Arc<dyn EventStore>,
    session_id: String,
    state: VerifiedSessionState,
    calls: Vec<ToolCall>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    tokio::task::spawn_blocking(move || {
        append_interrupted_tool_results_blocking(&*store, &session_id, state, &calls)
    })
    .await
    .map_err(|_| "tool_interruption_join")?
}

fn append_interrupted_tool_results_blocking(
    store: &dyn EventStore,
    session_id: &str,
    mut state: VerifiedSessionState,
    calls: &[ToolCall],
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    for _ in 0..16 {
        let events = calls
            .iter()
            .filter(|call| state.inflight_tool_call_ids.contains(&call.tool_call_id))
            .map(|call| {
                EventDraft::identified(|message_id| SessionEvent::MessageAppended {
                    message: TranscriptMessage {
                        message_id: message_id.to_owned(),
                        role: TranscriptRole::Tool,
                        content: TOOL_INTERRUPTED_MESSAGE.into(),
                        is_error: true,
                        tool_call_id: Some(call.tool_call_id.clone()),
                        tool_calls: Vec::new(),
                        provider_context: None,
                        source_mailbox_seq: None,
                    },
                    wake_wait: false,
                })
            })
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Ok((replayed_append(session_id, &state), state));
        }
        match store.append_verified(session_id, state, &events) {
            Ok(appended) => return Ok((appended.append, appended.state)),
            Err(StoreError::OptimisticConcurrency { .. }) => {
                state = store
                    .rehydrate_verified(session_id)
                    .map_err(|_| "tool_interruption_rehydrate")?;
            }
            Err(_) => return Err("tool_interruption_append"),
        }
    }
    Err("tool_interruption_concurrency")
}

fn append_tool_result_blocking(
    store: &dyn EventStore,
    session_id: &str,
    mut state: VerifiedSessionState,
    call: &ToolCall,
    result: &ToolExecutionResult,
    control: &ToolResultControl,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    for _ in 0..16 {
        if !state.inflight_tool_call_ids.contains(&call.tool_call_id) {
            if state.transcript.iter().any(|message| {
                message.role == TranscriptRole::Tool
                    && message.tool_call_id.as_deref() == Some(call.tool_call_id.as_str())
            }) {
                return Ok((replayed_append(session_id, &state), state));
            }
            return Err("tool_result_without_inflight_call");
        }
        let event_count = match control {
            ToolResultControl::Continue => 1,
            ToolResultControl::Wait(_) => 3,
            ToolResultControl::FinishActivation { .. } => 2,
        };
        let events = EventDraft::batch(event_count, |event_ids| {
            let mut events = vec![SessionEvent::MessageAppended {
                message: TranscriptMessage {
                    message_id: event_ids[0].clone(),
                    role: TranscriptRole::Tool,
                    content: result.content.clone().into(),
                    is_error: result.is_error,
                    tool_call_id: Some(call.tool_call_id.clone()),
                    tool_calls: Vec::new(),
                    provider_context: None,
                    source_mailbox_seq: None,
                },
                wake_wait: false,
            }];
            match control {
                ToolResultControl::Continue => {}
                ToolResultControl::Wait(wait) => {
                    let wait_id = event_ids[1].clone();
                    events.push(SessionEvent::WaitSet {
                        wait: ActiveWait {
                            wait_id: wait_id.clone(),
                            reason: wait.reason.clone(),
                            timeout_seconds: wait.timeout_seconds,
                            deadline_ms: wait.deadline_ms,
                            source: WaitSource::WaitFor,
                            tool_call_ids: wait.tool_call_ids.clone(),
                        },
                    });
                    events.push(SessionEvent::WaitTimerScheduled {
                        timer: crate::session::state::WaitTimerIntent {
                            wait_id,
                            deadline_ms: wait.deadline_ms,
                        },
                    });
                }
                ToolResultControl::FinishActivation {
                    activation_id,
                    finished_at_ms,
                } => events.push(SessionEvent::ActivationFinished {
                    activation_id: activation_id.clone(),
                    outcome: ActivationOutcome::Finished,
                    finished_at_ms: *finished_at_ms,
                }),
            }
            events
        });
        match store.append_verified(session_id, state, &events) {
            Ok(appended) => return Ok((appended.append, appended.state)),
            Err(StoreError::OptimisticConcurrency { .. }) => {
                state = store
                    .rehydrate_verified(session_id)
                    .map_err(|_| "tool_result_rehydrate")?;
            }
            Err(_) => return Err("tool_result_append"),
        }
    }
    Err("tool_result_concurrency")
}

pub(super) enum ToolResultControl {
    Continue,
    Wait(PendingWait),
    FinishActivation {
        activation_id: String,
        finished_at_ms: i64,
    },
}

pub(super) struct PendingWait {
    reason: String,
    timeout_seconds: u32,
    deadline_ms: i64,
    tool_call_ids: Vec<String>,
}

pub(super) fn parse_wait(call: &ToolCall, clock: &dyn Clock) -> Result<PendingWait, &'static str> {
    let object = call.arguments.as_object().ok_or("wait input")?;
    if object
        .keys()
        .any(|key| key != "reason" && key != "timeout_seconds")
    {
        return Err("wait input");
    }
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty())
        .ok_or("wait reason")?;
    let timeout_seconds = object
        .get("timeout_seconds")
        .map(|value| value.as_u64().ok_or("wait timeout"))
        .transpose()?
        .unwrap_or(60);
    let timeout_seconds = u32::try_from(timeout_seconds).map_err(|_| "wait timeout")?;
    if !(WAIT_MIN_SECONDS..=WAIT_MAX_SECONDS).contains(&timeout_seconds) {
        return Err("wait timeout");
    }
    let deadline_ms = clock
        .now_ms()
        .checked_add(i64::from(timeout_seconds) * 1_000)
        .ok_or("wait deadline")?;
    Ok(PendingWait {
        reason: reason.to_owned(),
        timeout_seconds,
        deadline_ms,
        tool_call_ids: vec![call.tool_call_id.clone()],
    })
}

pub(super) fn replayed_append(session_id: &str, state: &SessionState) -> AppendResult {
    AppendResult {
        stream_id: session_id.to_owned(),
        events: Vec::new(),
        stream_version: state.stream_version,
        replayed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{runtime::ports::OutstandingWaitTimer, store::JsonlEventStore};

    struct RejectingAppendStore;

    impl StorePort for RejectingAppendStore {
        fn create_session(
            &self,
            _create: &SessionCreate,
        ) -> Result<SessionCreateResult, StoreError> {
            unreachable!()
        }

        fn append(
            &self,
            _stream_id: &str,
            _current: &SessionState,
            _events: &[EventDraft],
        ) -> Result<SessionAppendResult, StoreError> {
            unreachable!()
        }

        fn append_verified(
            &self,
            _stream_id: &str,
            _current: VerifiedSessionState,
            _events: &[EventDraft],
        ) -> Result<SessionAppendResult, StoreError> {
            Err(StoreError::InvalidSessionStream)
        }

        fn rehydrate(&self, _stream_id: &str) -> Result<SessionState, RehydrateError> {
            unreachable!()
        }

        fn rehydrate_verified(
            &self,
            _stream_id: &str,
        ) -> Result<VerifiedSessionState, RehydrateError> {
            unreachable!()
        }

        fn read_stream(
            &self,
            _stream_id: &str,
            _after_version: crate::session::state::StreamVersion,
            _limit: usize,
        ) -> Result<Vec<EventRecord>, StoreError> {
            unreachable!()
        }

        fn read_stream_before(
            &self,
            _stream_id: &str,
            _before_event_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<EventRecord>, StoreError> {
            unreachable!()
        }

        fn read_event(
            &self,
            _stream_id: &str,
            _event_id: &str,
        ) -> Result<Option<EventRecord>, StoreError> {
            unreachable!()
        }

        fn list_outstanding_wait_timers(&self) -> Result<Vec<OutstandingWaitTimer>, StoreError> {
            unreachable!()
        }

        fn list_runnable_sessions(&self) -> Result<Vec<SessionRef>, StoreError> {
            unreachable!()
        }

        fn list_active_activations(&self) -> Result<Vec<SessionRef>, StoreError> {
            unreachable!()
        }

        fn list_sessions(&self, _limit: usize) -> Result<Vec<SessionListItem>, StoreError> {
            unreachable!()
        }

        fn list_sessions_page(
            &self,
            _cursor: Option<&SessionListCursor>,
            _limit: usize,
        ) -> Result<SessionListPage, StoreError> {
            unreachable!()
        }

        fn append_handoff_verified(
            &self,
            _stream_id: &str,
            _current: VerifiedSessionState,
            _events: &[EventDraft],
        ) -> Result<SessionAppendResult, StoreError> {
            unreachable!()
        }
    }

    #[test]
    fn model_result_append_failure_retains_the_store_error() {
        let temporary = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(temporary.path().join("sessions")).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: SessionSelection {
                    profile_id: "profile".to_owned(),
                    model: "model".to_owned(),
                    thinking: "off".to_owned(),
                },
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id;
        let state = store.rehydrate_verified(&session_id).unwrap();
        let error = append_model_result_blocking(
            &RejectingAppendStore,
            &session_id,
            state,
            &ModelResultInput {
                identity: PreparedRequestIdentity {
                    activation_id: "activation".to_owned(),
                    round_id: "round".to_owned(),
                    request_id: "request".to_owned(),
                    maximum_attempts: 1,
                    attempt_id: "attempt".to_owned(),
                    attempt_number: 1,
                },
                attempt_id: "attempt".to_owned(),
                usage: None,
                assistant_content: String::new(),
                provider_context: None,
                tool_calls: Vec::new(),
                provider_input: None,
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "model_result_append: session stream is inconsistent with its creation event"
        );
    }
}
