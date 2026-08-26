use super::*;

pub(super) async fn append_model_attempt_failure_with_error(
    store: Arc<dyn EventStore>,
    session_id: String,
    trigger_message_id: String,
    error_class: ModelAttemptErrorClass,
    error_message: &'static str,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    tokio::task::spawn_blocking(move || {
        append_model_attempt_failure_blocking(
            &*store,
            &session_id,
            &trigger_message_id,
            error_class,
            error_message,
        )
    })
    .await
    .map_err(|_| "model_failure_join")?
}

pub(super) fn append_model_attempt_failure_blocking(
    store: &dyn EventStore,
    session_id: &str,
    trigger_message_id: &str,
    error_class: ModelAttemptErrorClass,
    error_message: &str,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let failure = ModelAttemptFailure {
        trigger_message_id: trigger_message_id.to_owned(),
        error: ModelAttemptError {
            class: error_class,
            message: error_message.to_owned(),
        },
    };
    for _ in 0..16 {
        let state = store
            .rehydrate_verified(session_id)
            .map_err(|_| "model_failure_rehydrate")?;
        if let Some(existing) = state.terminal_model_failure_for_last_input() {
            if existing == &failure {
                let append = replayed_append(session_id, &state);
                return Ok((append, state));
            }
            return Err("model_failure_conflict");
        }
        if !state.transcript.last().is_some_and(|message| {
            message.role.is_mailbox_input() && message.message_id == trigger_message_id
        }) {
            return Err("model_failure_trigger");
        }
        let events = EventDraft::single(SessionEvent::ModelAttemptFailed {
            failure: failure.clone(),
        });
        match store.append_verified(session_id, state, &events) {
            Ok(appended) => return Ok((appended.append, appended.state)),
            Err(StoreError::OptimisticConcurrency { .. }) => continue,
            Err(_) => return Err("model_failure_append"),
        }
    }
    Err("model_failure_concurrency")
}

pub(super) async fn rehydrate_verified(
    store: Arc<dyn EventStore>,
    session_id: String,
) -> Result<VerifiedSessionState, &'static str> {
    tokio::task::spawn_blocking(move || {
        store
            .rehydrate_verified(&session_id)
            .map_err(|_| "rehydrate_store")
    })
    .await
    .map_err(|_| "rehydrate_join")?
}

#[derive(Clone, Debug)]
pub(super) struct PreparedRequestIdentity {
    pub(super) activation_id: String,
    pub(super) round_id: String,
    pub(super) request_id: String,
    pub(super) maximum_attempts: u32,
    pub(super) attempt_id: String,
    pub(super) attempt_number: u32,
}

pub(super) struct PreparedModelExecution {
    pub(super) state: VerifiedSessionState,
    pub(super) completion: PreparedModelCompletion,
}

pub(super) enum PreparedModelCompletion {
    Completed {
        outcome: ModelOutcome,
        attempt_id: String,
    },
    MailboxPending,
    Terminal,
}

pub(super) struct ModelRoundInput<'a> {
    pub(super) state: &'a VerifiedSessionState,
    pub(super) selection: &'a SessionSelection,
    pub(super) request: &'a UnboundModelRequest,
    pub(super) purpose: ModelRequestPurpose,
    pub(super) maximum_attempts: u32,
}

pub(super) struct ModelFailureInput<'a> {
    pub(super) identity: &'a PreparedRequestIdentity,
    pub(super) attempt_id: &'a str,
    pub(super) attempt_number: u32,
    pub(super) error_class: &'a str,
    pub(super) retryable: bool,
    pub(super) provider_input: Option<ProviderInputDiagnostics>,
}

#[derive(Clone)]
struct ModelRequestDeclaration {
    request_fingerprint: String,
    prompt_fingerprint: String,
    tool_schema_fingerprint: String,
    maximum_attempts: u32,
}

impl ModelRequestDeclaration {
    fn new(
        selection: &SessionSelection,
        request: &UnboundModelRequest,
        maximum_attempts: u32,
    ) -> Result<Self, &'static str> {
        let selection_fingerprint = stable_fingerprint(
            "model-selection",
            &serde_json::to_string(selection).map_err(|_| "model_selection_fingerprint")?,
        );
        let prompt_fingerprint = request.prompt_fingerprint.clone();
        let tool_schema_fingerprint = request.tool_schema_fingerprint.clone();
        let request_fingerprint = stable_fingerprint(
            "model-request",
            &format!(
                "{selection_fingerprint}:{prompt_fingerprint}:{tool_schema_fingerprint}:{:?}",
                request.max_output_tokens,
            ),
        );
        Ok(Self {
            request_fingerprint,
            prompt_fingerprint,
            tool_schema_fingerprint,
            maximum_attempts,
        })
    }

    fn event(&self, activation_id: &str, round_id: &str, request_id: &str) -> SessionEvent {
        SessionEvent::ModelRequestDeclared {
            activation_id: activation_id.to_owned(),
            round_id: round_id.to_owned(),
            request_id: request_id.to_owned(),
            request_fingerprint: self.request_fingerprint.clone(),
            prompt_fingerprint: self.prompt_fingerprint.clone(),
            tool_schema_fingerprint: self.tool_schema_fingerprint.clone(),
            maximum_attempts: self.maximum_attempts,
        }
    }

    fn matches(
        &self,
        fact: &crate::session::state::ModelRequestFact,
        activation_id: &str,
        round_id: &str,
    ) -> bool {
        fact.activation_id == activation_id
            && fact.round_id == round_id
            && fact.request_fingerprint == self.request_fingerprint
            && fact.prompt_fingerprint == self.prompt_fingerprint
            && fact.tool_schema_fingerprint == self.tool_schema_fingerprint
            && fact.maximum_attempts == self.maximum_attempts
    }
}

fn reconstructed_request_matches(
    fact: &crate::session::state::ModelRequestFact,
    selection: &SessionSelection,
    activation_id: &str,
    round_id: &str,
    request: &UnboundModelRequest,
) -> Result<bool, &'static str> {
    let declaration = ModelRequestDeclaration::new(selection, request, fact.maximum_attempts)?;
    Ok(declaration.matches(fact, activation_id, round_id))
}

pub(super) struct ContextHandoffPlanInput<'a> {
    pub(super) state: &'a VerifiedSessionState,
    pub(super) plan: ContextHandoffPlanDraft,
    pub(super) request: &'a UnboundModelRequest,
    pub(super) maximum_attempts: u32,
}

pub(super) async fn append_context_handoff_plan(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    input: ContextHandoffPlanInput<'_>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let ContextHandoffPlanInput {
        state,
        plan,
        request,
        maximum_attempts,
    } = input;
    if state.active_model_round.as_ref().is_some_and(|round| {
        round.attempt.as_ref().is_none_or(|attempt| {
            attempt.outcome != crate::session::state::ModelAttemptOutcome::Completed
        })
    }) {
        return Err("context_handoff_active_round");
    }
    let declaration = ModelRequestDeclaration::new(&plan.selection, request, maximum_attempts)?;
    let started_at_ms = clock.now_ms();
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let current = store
                .rehydrate_verified(&session_id)
                .map_err(|_| "context_handoff_prepare_rehydrate")?;
            let already_prepared =
                current
                    .pending_context_handoff
                    .as_ref()
                    .is_some_and(|pending| {
                        pending.activation_id == plan.activation_id
                            && pending.previous_handoff_id == plan.previous_handoff_id
                            && pending.next_generation == plan.next_generation
                            && pending.covered_through_message_id == plan.covered_through_message_id
                            && pending.max_output_tokens == plan.max_output_tokens
                            && pending.selection == plan.selection
                            && current.active_model_round.as_ref().is_some_and(|round| {
                                round.purpose == ModelRequestPurpose::ContextHandoff
                                    && round.request.as_ref().is_some_and(|prepared| {
                                        declaration.matches(
                                            prepared,
                                            &pending.activation_id,
                                            &round.round_id,
                                        )
                                    })
                            })
                    });
            if already_prepared {
                let append = replayed_append(&session_id, &current);
                return Ok((append, current));
            }
            let mailbox_through_seq = current.consumed_through_mailbox_seq;
            let events = EventDraft::batch(3, |event_ids| {
                vec![
                    SessionEvent::ContextHandoffPlanned {
                        plan: plan.commit(event_ids[0].clone()),
                    },
                    SessionEvent::ModelRoundStarted {
                        activation_id: plan.activation_id.clone(),
                        round_id: event_ids[1].clone(),
                        purpose: ModelRequestPurpose::ContextHandoff,
                        mailbox_through_seq,
                        started_at_ms,
                    },
                    declaration.event(&plan.activation_id, &event_ids[1], &event_ids[2]),
                ]
            });
            match store.append_verified(&session_id, current, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(_) => return Err("context_handoff_prepare_append"),
            }
        }
        Err("context_handoff_prepare_concurrency")
    })
    .await
    .map_err(|_| "context_handoff_prepare_join")?
}

pub(super) async fn append_runtime_event(
    store: Arc<dyn EventStore>,
    session_id: String,
    event: SessionEvent,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let state = rehydrate_verified(store.clone(), session_id.clone()).await?;
    append_runtime_event_from_state(store, session_id, state, event).await
}

pub(super) async fn append_runtime_event_from_state(
    store: Arc<dyn EventStore>,
    session_id: String,
    state: VerifiedSessionState,
    event: SessionEvent,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    append_runtime_events_from_state(store, session_id, state, vec![event]).await
}

pub(super) async fn append_runtime_events(
    store: Arc<dyn EventStore>,
    session_id: String,
    events: Vec<SessionEvent>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let state = rehydrate_verified(store.clone(), session_id.clone()).await?;
    append_runtime_events_from_state(store, session_id, state, events).await
}

pub(super) async fn append_runtime_events_from_state(
    store: Arc<dyn EventStore>,
    session_id: String,
    mut state: VerifiedSessionState,
    events: Vec<SessionEvent>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let events = EventDraft::many(events.clone());
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => {
                    state = store
                        .rehydrate_verified(&session_id)
                        .map_err(|_| "runtime_event_rehydrate")?;
                }
                Err(_) => {
                    return Err("runtime_event_append");
                }
            }
        }
        Err("runtime_event_concurrency")
    })
    .await
    .map_err(|_| "runtime_event_join")?
}

pub(super) async fn append_runtime_drafts_from_state<F>(
    store: Arc<dyn EventStore>,
    session_id: String,
    mut state: VerifiedSessionState,
    build: F,
) -> Result<(AppendResult, VerifiedSessionState), &'static str>
where
    F: Fn(&SessionState) -> Result<Vec<EventDraft>, &'static str> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let events = build(&state)?;
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => {
                    state = store
                        .rehydrate_verified(&session_id)
                        .map_err(|_| "runtime_event_rehydrate")?;
                }
                Err(_) => return Err("runtime_event_append"),
            }
        }
        Err("runtime_event_concurrency")
    })
    .await
    .map_err(|_| "runtime_event_join")?
}

pub(super) async fn append_expired_timer(
    store: Arc<dyn EventStore>,
    session_id: String,
    wait_id: String,
    now_ms: i64,
) -> Result<Option<(AppendResult, VerifiedSessionState)>, &'static str> {
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let state = store
                .rehydrate_verified(&session_id)
                .map_err(|_| "timer_rehydrate")?;
            let Some(timer) = state.active_timer.clone() else {
                return Ok(None);
            };
            if timer.wait_id != wait_id
                || timer.deadline_ms > now_ms
                || state
                    .active_wait
                    .as_ref()
                    .is_none_or(|wait| wait.wait_id != timer.wait_id)
                || state.wake_pending_wait_id.as_deref() == Some(timer.wait_id.as_str())
            {
                return Ok(None);
            }
            let events = EventDraft::single(SessionEvent::WaitExpired {
                wait_id: timer.wait_id,
            });
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok(Some((appended.append, appended.state))),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(_) => return Err("timer_append"),
            }
        }
        Err("timer_concurrency")
    })
    .await
    .map_err(|_| "timer_join")?
}

pub(super) async fn start_activation(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    state: &VerifiedSessionState,
) -> Result<Option<(AppendResult, VerifiedSessionState)>, &'static str> {
    let started_at_ms = clock.now_ms();
    let mut state = state.clone();
    tokio::task::spawn_blocking(move || {
        // Claim, ActivationStarted, and already-queued first-boundary
        // materialize must share one expected-version append. A crash after
        // start-only would otherwise leave the session active with those
        // deliveries still unmaterialized.
        for _ in 0..16 {
            if state.active_activation.is_some() {
                let append = replayed_append(&session_id, &state);
                return Ok(Some((append, state)));
            }
            if !state.is_startup_runnable() {
                return Ok(None);
            }
            let events = vec![EventDraft::identified(|activation_id| {
                SessionEvent::ActivationStarted {
                    activation_id: activation_id.to_owned(),
                    selection: state.selection.clone(),
                    started_at_ms,
                }
            })];
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok(Some((appended.append, appended.state))),
                Err(StoreError::OptimisticConcurrency { .. }) => {
                    state = store
                        .rehydrate_verified(&session_id)
                        .map_err(|_| "activation_start_rehydrate")?;
                }
                Err(_) => return Err("activation_start_append"),
            }
        }
        Err("activation_start_concurrency")
    })
    .await
    .map_err(|_| "activation_start_join")?
}

pub(super) async fn finish_activation(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    state: &VerifiedSessionState,
    activation_id: String,
    outcome: ActivationOutcome,
) -> Result<Option<(AppendResult, VerifiedSessionState)>, &'static str> {
    if state.active_activation.is_none() {
        return Ok(None);
    }
    Ok(Some(
        append_runtime_event_from_state(
            store,
            session_id,
            state.clone(),
            SessionEvent::ActivationFinished {
                activation_id,
                outcome,
                finished_at_ms: clock.now_ms(),
            },
        )
        .await?,
    ))
}

pub(super) async fn prepare_model_round(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    input: ModelRoundInput<'_>,
) -> Result<
    (
        Vec<(AppendResult, VerifiedSessionState)>,
        VerifiedSessionState,
        PreparedRequestIdentity,
    ),
    &'static str,
> {
    let ModelRoundInput {
        state,
        selection,
        request,
        purpose,
        maximum_attempts,
    } = input;
    let mut appends = Vec::new();
    let mut current = state.clone();
    let activation = current
        .active_activation
        .as_ref()
        .ok_or("model_round_without_activation")?;
    let activation_id = activation.activation_id.clone();
    let needs_new_round = current.active_model_round.as_ref().is_none_or(|round| {
        round.attempt.as_ref().is_some_and(|attempt| {
            attempt.outcome == crate::session::state::ModelAttemptOutcome::Completed
        })
    });
    let declaration = ModelRequestDeclaration::new(selection, request, maximum_attempts)?;
    if needs_new_round {
        let build_activation_id = activation_id.clone();
        let build_purpose = purpose.clone();
        let build_declaration = declaration.clone();
        let started_at_ms = clock.now_ms();
        let append = append_runtime_drafts_from_state(
            store.clone(),
            session_id.clone(),
            current,
            move |state| {
                let mailbox_through_seq = state.consumed_through_mailbox_seq;
                Ok(EventDraft::batch(2, |event_ids| {
                    vec![
                        SessionEvent::ModelRoundStarted {
                            activation_id: build_activation_id.clone(),
                            round_id: event_ids[0].clone(),
                            purpose: build_purpose.clone(),
                            mailbox_through_seq,
                            started_at_ms,
                        },
                        build_declaration.event(&build_activation_id, &event_ids[0], &event_ids[1]),
                    ]
                }))
            },
        )
        .await?;
        current = append.1.clone();
        appends.push(append);
    } else {
        let round = current
            .active_model_round
            .as_ref()
            .ok_or("model_round_missing")?;
        if round.purpose != purpose {
            return Err("model_round_purpose");
        }
        if let Some(prepared) = &round.request {
            if !reconstructed_request_matches(
                prepared,
                selection,
                &activation_id,
                &round.round_id,
                request,
            )? {
                return Err("model_request_conflict");
            }
        } else {
            let build_activation_id = activation_id.clone();
            let build_round_id = round.round_id.clone();
            let build_declaration = declaration.clone();
            let append = append_runtime_drafts_from_state(
                store.clone(),
                session_id.clone(),
                current,
                move |_| {
                    Ok(vec![EventDraft::identified(|request_id| {
                        build_declaration.event(&build_activation_id, &build_round_id, request_id)
                    })])
                },
            )
            .await?;
            current = append.1.clone();
            appends.push(append);
        }
    }
    let round = current
        .active_model_round
        .as_ref()
        .ok_or("model_round_missing_after_prepare")?;
    if round.purpose != purpose {
        return Err("model_round_purpose");
    }
    let round_id = round.round_id.clone();
    let maximum_attempts = round
        .request
        .as_ref()
        .map(|request| request.maximum_attempts)
        .ok_or("model_request_missing_after_prepare")?;
    let request_id = round
        .request
        .as_ref()
        .map(|request| request.request_id.clone())
        .ok_or("model_request_missing_after_prepare")?;
    let attempt_number = round
        .retry
        .as_ref()
        .map(|schedule| schedule.next_attempt_number)
        .unwrap_or(1);
    if round.attempt.is_none() {
        let build_activation_id = activation_id.clone();
        let build_round_id = round_id.clone();
        let build_request_id = request_id.clone();
        let started_at_ms = clock.now_ms();
        let append = append_runtime_drafts_from_state(store, session_id, current, move |_| {
            Ok(vec![EventDraft::identified(|attempt_id| {
                SessionEvent::ModelAttemptStarted {
                    activation_id: build_activation_id.clone(),
                    round_id: build_round_id.clone(),
                    request_id: build_request_id.clone(),
                    attempt_id: attempt_id.to_owned(),
                    attempt_number,
                    started_at_ms,
                }
            })])
        })
        .await?;
        current = append.1.clone();
        appends.push(append);
    }
    let attempt = current
        .active_model_round
        .as_ref()
        .and_then(|round| round.attempt.as_ref())
        .ok_or("model_attempt_missing_after_prepare")?;
    let attempt_id = attempt.attempt_id.clone();
    Ok((
        appends,
        current,
        PreparedRequestIdentity {
            activation_id,
            round_id,
            request_id,
            maximum_attempts,
            attempt_id,
            attempt_number,
        },
    ))
}

pub(super) async fn append_context_handoff_document(
    store: Arc<dyn EventStore>,
    session_id: String,
    identity: &PreparedRequestIdentity,
    attempt_id: &str,
    handoff: ContextHandoffDocumentDraft,
    usage: Option<ModelUsageAnchor>,
    provider_input: Option<ProviderInputDiagnostics>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let identity = identity.clone();
    let attempt_id = attempt_id.to_owned();
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let state = store
                .rehydrate_verified(&session_id)
                .map_err(|_| "context_handoff_rehydrate")?;
            let events = EventDraft::batch(2, |event_ids| {
                let usage = usage.clone().map(|mut usage| {
                    usage.result_event_id = Some(event_ids[0].clone());
                    usage
                });
                vec![
                    SessionEvent::ContextHandoffCreated {
                        handoff: handoff.commit(event_ids[0].clone()),
                    },
                    SessionEvent::ModelRequestCompleted {
                        activation_id: identity.activation_id.clone(),
                        round_id: identity.round_id.clone(),
                        request_id: identity.request_id.clone(),
                        attempt_id: attempt_id.clone(),
                        usage,
                        provider_input: provider_input.clone(),
                    },
                ]
            });
            match store.append_handoff_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(_) => return Err("context_handoff_append"),
            }
        }
        Err("context_handoff_concurrency")
    })
    .await
    .map_err(|_| "context_handoff_join")?
}

pub(super) struct ContextHandoffRejectionInput {
    pub(super) identity: PreparedRequestIdentity,
    pub(super) attempt_id: String,
    pub(super) plan_id: String,
    pub(super) usage: Option<ModelUsageAnchor>,
    pub(super) assistant_content: String,
    pub(super) tool_calls: Vec<ToolCall>,
    pub(super) provider_context: Option<ProviderContext>,
    pub(super) provider_input: Option<ProviderInputDiagnostics>,
}

pub(super) async fn append_context_handoff_rejection(
    store: Arc<dyn EventStore>,
    session_id: String,
    input: ContextHandoffRejectionInput,
) -> Result<(AppendResult, VerifiedSessionState), String> {
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let state = store
                .rehydrate_verified(&session_id)
                .map_err(|error| format!("context_handoff_rejection_rehydrate: {error}"))?;
            let events = EventDraft::many(vec![
                SessionEvent::ModelRequestCompleted {
                    activation_id: input.identity.activation_id.clone(),
                    round_id: input.identity.round_id.clone(),
                    request_id: input.identity.request_id.clone(),
                    attempt_id: input.attempt_id.clone(),
                    usage: input.usage.clone(),
                    provider_input: input.provider_input.clone(),
                },
                SessionEvent::ContextHandoffRejected {
                    plan_id: input.plan_id.clone(),
                    assistant_content: input.assistant_content.clone(),
                    tool_calls: input.tool_calls.clone(),
                    provider_context: input.provider_context.clone(),
                },
            ]);
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(error) => return Err(format!("context_handoff_rejection_append: {error}")),
            }
        }
        Err("context_handoff_rejection_concurrency".to_owned())
    })
    .await
    .map_err(|error| format!("context_handoff_rejection_join: {error}"))?
}

pub(super) async fn append_context_handoff_failure(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    state: &VerifiedSessionState,
    message: &'static str,
    completed_request: Option<(&PreparedRequestIdentity, &str, Option<ModelUsageAnchor>)>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let plan = state
        .pending_context_handoff
        .as_ref()
        .cloned()
        .ok_or("context_handoff_plan_missing")?;
    let plan_id = plan.plan_id;
    let activation_id = plan.activation_id;
    let completed_request = completed_request
        .map(|(identity, attempt_id, usage)| (identity.clone(), attempt_id.to_owned(), usage));
    let error = ModelAttemptError {
        class: ModelAttemptErrorClass::ContextHandoffFailed,
        message: message.to_owned(),
    };
    let finished_at_ms = clock.now_ms();
    tokio::task::spawn_blocking(move || {
        for _ in 0..16 {
            let state = store
                .rehydrate_verified(&session_id)
                .map_err(|_| "context_handoff_failure_rehydrate")?;
            if state.active_activation.is_none()
                && state.pending_context_handoff.is_none()
                && state.last_context_handoff_failure.as_ref() == Some(&error)
            {
                let append = replayed_append(&session_id, &state);
                return Ok((append, state));
            }
            let mut events = Vec::with_capacity(3);
            if let Some((identity, attempt_id, usage)) = &completed_request {
                events.push(SessionEvent::ModelRequestCompleted {
                    activation_id: identity.activation_id.clone(),
                    round_id: identity.round_id.clone(),
                    request_id: identity.request_id.clone(),
                    attempt_id: attempt_id.clone(),
                    usage: usage.clone(),
                    provider_input: None,
                });
            }
            events.push(SessionEvent::ContextHandoffFailed {
                plan_id: plan_id.clone(),
                error: error.clone(),
                finished_at_ms,
            });
            events.push(SessionEvent::ActivationFinished {
                activation_id: activation_id.clone(),
                outcome: ActivationOutcome::Failed,
                finished_at_ms,
            });
            let events = EventDraft::many(events);
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(_) => return Err("context_handoff_failure_append"),
            }
        }
        Err("context_handoff_failure_concurrency")
    })
    .await
    .map_err(|_| "context_handoff_failure_join")?
}

pub(super) async fn append_model_lifecycle_failure(
    store: Arc<dyn EventStore>,
    session_id: String,
    input: ModelFailureInput<'_>,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let ModelFailureInput {
        identity,
        attempt_id,
        attempt_number,
        error_class,
        retryable,
        provider_input,
    } = input;
    let append = append_runtime_event(
        store,
        session_id,
        SessionEvent::ModelAttemptFailedFact {
            activation_id: identity.activation_id.clone(),
            round_id: identity.round_id.clone(),
            request_id: identity.request_id.clone(),
            attempt_id: attempt_id.to_owned(),
            attempt_number,
            error_class: error_class.to_owned(),
            retryable,
            provider_input,
        },
    )
    .await?;
    Ok(append)
}

pub(super) async fn append_model_attempts_exhausted(
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    session_id: String,
    identity: &PreparedRequestIdentity,
    attempt_id: &str,
    attempt_number: u32,
) -> Result<(AppendResult, VerifiedSessionState), &'static str> {
    let activation_id = identity.activation_id.clone();
    let round_id = identity.round_id.clone();
    let request_id = identity.request_id.clone();
    let maximum_attempts = identity.maximum_attempts;
    let attempt_id = attempt_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let matches_fact = |state: &SessionState| {
            state
                .last_model_attempts_exhausted
                .as_ref()
                .is_some_and(|fact| {
                    fact.activation_id == activation_id
                        && fact.round_id == round_id
                        && fact.request_id == request_id
                        && fact.attempt_id == attempt_id
                        && fact.attempt_number == attempt_number
                        && fact.maximum_attempts == maximum_attempts
                })
        };
        for _ in 0..16 {
            let state = store
                .rehydrate_verified(&session_id)
                .map_err(|_| "model_exhaustion_rehydrate")?;
            if matches_fact(&state) {
                let append = replayed_append(&session_id, &state);
                return Ok((append, state));
            }
            let fact = crate::session::state::ModelAttemptsExhaustedFact {
                activation_id: activation_id.clone(),
                round_id: round_id.clone(),
                request_id: request_id.clone(),
                attempt_id: attempt_id.clone(),
                attempt_number,
                maximum_attempts,
                finished_at_ms: clock.now_ms(),
            };
            let events = EventDraft::single(SessionEvent::ModelAttemptsExhausted { fact });
            match store.append_verified(&session_id, state, &events) {
                Ok(appended) => return Ok((appended.append, appended.state)),
                Err(StoreError::OptimisticConcurrency { .. }) => continue,
                Err(_) => return Err("model_exhaustion_append"),
            }
        }
        Err("model_exhaustion_concurrency")
    })
    .await
    .map_err(|_| "model_exhaustion_join")?
}

pub(super) fn model_error_class(error: &ModelError) -> &'static str {
    match error {
        ModelError::Unavailable => "provider_unavailable",
        ModelError::InvalidSelection => "invalid_selection",
        ModelError::ProfileUnavailable => "profile_unavailable",
        ModelError::ProviderFailed(_) => "provider_failed",
        ModelError::InvalidToolArguments => "invalid_tool_arguments",
    }
}

pub(super) fn terminal_model_error(error: &ModelError) -> (ModelAttemptErrorClass, &'static str) {
    match error {
        ModelError::Unavailable => (
            ModelAttemptErrorClass::ProviderUnavailable,
            "model provider unavailable",
        ),
        ModelError::InvalidSelection => (
            ModelAttemptErrorClass::InvalidSelection,
            "invalid model selection",
        ),
        ModelError::ProfileUnavailable => (
            ModelAttemptErrorClass::ProfileUnavailable,
            "profile unavailable",
        ),
        ModelError::ProviderFailed(_) => (
            ModelAttemptErrorClass::ProviderFailed,
            "model provider request failed",
        ),
        ModelError::InvalidToolArguments => (
            ModelAttemptErrorClass::InvalidToolArguments,
            "model supplied invalid tool arguments",
        ),
    }
}

pub(super) fn terminal_model_error_class(
    error_class: &str,
) -> Result<(ModelAttemptErrorClass, &'static str), &'static str> {
    Ok(match error_class {
        "provider_unavailable" => (
            ModelAttemptErrorClass::ProviderUnavailable,
            "model provider unavailable",
        ),
        "invalid_selection" => (
            ModelAttemptErrorClass::InvalidSelection,
            "invalid model selection",
        ),
        "profile_unavailable" => (
            ModelAttemptErrorClass::ProfileUnavailable,
            "profile unavailable",
        ),
        "provider_failed" => (
            ModelAttemptErrorClass::ProviderFailed,
            "model provider request failed",
        ),
        "invalid_tool_arguments" => (
            ModelAttemptErrorClass::InvalidToolArguments,
            "model supplied invalid tool arguments",
        ),
        _ => return Err("invalid_model_error_class"),
    })
}

pub(super) fn retry_delay_ms(base: Duration, maximum: Duration, attempt_number: u32) -> u64 {
    let exponent = attempt_number.saturating_sub(1).min(16);
    let multiplier = 1u64 << exponent;
    base.as_millis()
        .saturating_mul(multiplier as u128)
        .min(maximum.as_millis())
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> SessionSelection {
        SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "max".to_owned(),
        }
    }

    fn request(prompt_fingerprint: &str) -> UnboundModelRequest {
        UnboundModelRequest {
            selection: selection(),
            transcript: Arc::new(Vec::new()),
            tools: Arc::new(Vec::new()),
            prompt_fingerprint: prompt_fingerprint.to_owned(),
            tool_schema_fingerprint: "tools".to_owned(),
            max_output_tokens: Some(128_000),
            stream_observer: Arc::new(SilentModelStreamObserver),
        }
    }

    #[test]
    fn resumed_round_requires_the_same_reconstructed_provider_request() {
        let selection = selection();
        let original = request("prompt-a");
        let event = ModelRequestDeclaration::new(&selection, &original, 3)
            .unwrap()
            .event("activation", "round", "request");
        let SessionEvent::ModelRequestDeclared {
            activation_id,
            round_id,
            request_id,
            request_fingerprint,
            prompt_fingerprint,
            tool_schema_fingerprint,
            maximum_attempts,
        } = event
        else {
            panic!("expected request declaration");
        };
        let fact = crate::session::state::ModelRequestFact {
            activation_id,
            round_id,
            request_id,
            request_fingerprint,
            prompt_fingerprint,
            tool_schema_fingerprint,
            maximum_attempts,
        };

        assert!(
            reconstructed_request_matches(&fact, &selection, "activation", "round", &original,)
                .unwrap()
        );
        assert!(!reconstructed_request_matches(
            &fact,
            &selection,
            "activation",
            "round",
            &request("prompt-b"),
        )
        .unwrap());
    }

    #[test]
    fn terminal_model_errors_keep_their_actual_class() {
        let cases = [
            (
                ModelError::Unavailable,
                ModelAttemptErrorClass::ProviderUnavailable,
                "model provider unavailable",
            ),
            (
                ModelError::InvalidSelection,
                ModelAttemptErrorClass::InvalidSelection,
                "invalid model selection",
            ),
            (
                ModelError::ProfileUnavailable,
                ModelAttemptErrorClass::ProfileUnavailable,
                "profile unavailable",
            ),
            (
                ModelError::ProviderFailed(ProviderFailure::new("test.provider", false, "failed")),
                ModelAttemptErrorClass::ProviderFailed,
                "model provider request failed",
            ),
            (
                ModelError::InvalidToolArguments,
                ModelAttemptErrorClass::InvalidToolArguments,
                "model supplied invalid tool arguments",
            ),
        ];

        for (error, expected_class, expected_message) in cases {
            assert_eq!(
                terminal_model_error(&error),
                (expected_class.clone(), expected_message)
            );
            assert_eq!(
                terminal_model_error_class(model_error_class(&error)),
                Ok((expected_class, expected_message))
            );
        }
    }
}
