use super::*;

#[derive(Debug)]
pub struct ModelRequest {
    pub session_id: String,
    pub activation_id: String,
    pub round_id: String,
    pub selection: SessionSelection,
    pub transcript: Arc<Vec<ProviderMessage>>,
    pub tools: Arc<Vec<ToolDefinition>>,
    pub max_output_tokens: Option<u32>,
    pub stream_observer: Arc<dyn ModelStreamObserver>,
}

pub(super) struct UnboundModelRequest {
    pub(super) selection: SessionSelection,
    pub(super) transcript: Arc<Vec<ProviderMessage>>,
    pub(super) tools: Arc<Vec<ToolDefinition>>,
    pub(super) prompt_fingerprint: String,
    pub(super) tool_schema_fingerprint: String,
    pub(super) max_output_tokens: Option<u32>,
    pub(super) stream_observer: Arc<dyn ModelStreamObserver>,
}

impl UnboundModelRequest {
    pub(super) fn bind(
        self,
        session_id: &str,
        activation_id: String,
        round_id: String,
    ) -> ModelRequest {
        ModelRequest {
            session_id: session_id.to_owned(),
            activation_id,
            round_id,
            selection: self.selection,
            transcript: self.transcript,
            tools: self.tools,
            max_output_tokens: self.max_output_tokens,
            stream_observer: self.stream_observer,
        }
    }
}

pub(super) struct PreparedConversationContext {
    transcript: Arc<Vec<ProviderMessage>>,
    tools: Arc<Vec<ToolDefinition>>,
    estimated_input_tokens: u64,
    selection_fingerprint: String,
    prompt_fingerprint: String,
    tool_schema_fingerprint: String,
}

#[derive(Clone, Debug)]
pub struct ModelOutcome {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub provider_context: Option<ProviderContext>,
    pub usage: Option<ModelTokenUsage>,
    pub provider_input: Option<Box<ProviderInputDiagnostics>>,
}

#[derive(Clone, Debug)]
pub struct ModelTokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: u64,
    pub output_reasoning_tokens: Option<u64>,
    pub output_text_tokens: Option<u64>,
}

struct PreparedModelRequestInput<'a> {
    session_id: &'a str,
    state: VerifiedSessionState,
    request: ModelRequest,
    identity: &'a PreparedRequestIdentity,
    purpose: ModelRequestPurpose,
}

impl Runtime {
    async fn abandon_model_request_for_mailbox(
        self: &Arc<Self>,
        session_id: &str,
        state: VerifiedSessionState,
        identity: &PreparedRequestIdentity,
        attempt_id: &str,
        attempt_number: u32,
        interrupt_running_attempt: bool,
    ) -> Result<VerifiedSessionState, &'static str> {
        let reason = "mailbox_input_before_provider_invocation";
        let mut events = Vec::with_capacity(if interrupt_running_attempt { 2 } else { 1 });
        if interrupt_running_attempt {
            events.push(SessionEvent::ModelAttemptInterrupted {
                activation_id: identity.activation_id.clone(),
                round_id: identity.round_id.clone(),
                request_id: identity.request_id.clone(),
                attempt_id: attempt_id.to_owned(),
                attempt_number,
                reason: reason.to_owned(),
            });
        }
        events.push(SessionEvent::ModelRequestAbandoned {
            activation_id: identity.activation_id.clone(),
            round_id: identity.round_id.clone(),
            request_id: identity.request_id.clone(),
            attempt_id: attempt_id.to_owned(),
            reason: reason.to_owned(),
            abandoned_at_ms: self.clock.now_ms(),
        });
        let abandoned = append_runtime_events_from_state(
            self.store.clone(),
            session_id.to_owned(),
            state,
            events,
        )
        .await?;
        self.observe_append(&abandoned.0, &abandoned.1).await;
        Ok(abandoned.1)
    }

    pub(super) async fn recover_model_round(
        self: &Arc<Self>,
        session_id: String,
        state: VerifiedSessionState,
    ) -> Result<VerifiedSessionState, &'static str> {
        let Some(round) = state.active_model_round.clone() else {
            return Ok(state);
        };
        let Some(attempt) = round.attempt.clone() else {
            return Ok(state);
        };
        if attempt.outcome == crate::session::state::ModelAttemptOutcome::Failed {
            return self
                .recover_failed_model_round(session_id, state, attempt)
                .await;
        }
        if attempt.outcome != crate::session::state::ModelAttemptOutcome::Running {
            return Ok(state);
        }
        let abandoned_at_ms = self.clock.now_ms();
        let recovered = append_runtime_events(
            self.store.clone(),
            session_id,
            vec![
                SessionEvent::ModelAttemptInterrupted {
                    activation_id: attempt.activation_id.clone(),
                    round_id: attempt.round_id.clone(),
                    request_id: attempt.request_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    attempt_number: attempt.attempt_number,
                    reason: "runtime_restarted".to_owned(),
                },
                SessionEvent::ModelRequestAbandoned {
                    activation_id: attempt.activation_id,
                    round_id: attempt.round_id,
                    request_id: attempt.request_id,
                    attempt_id: attempt.attempt_id,
                    reason: "runtime_restarted".to_owned(),
                    abandoned_at_ms,
                },
            ],
        )
        .await?;
        self.observe_append(&recovered.0, &recovered.1).await;
        Ok(recovered.1)
    }

    async fn recover_failed_model_round(
        self: &Arc<Self>,
        session_id: String,
        mut state: VerifiedSessionState,
        attempt: crate::session::state::ModelAttemptRecord,
    ) -> Result<VerifiedSessionState, &'static str> {
        let Some(request) = state
            .active_model_round
            .as_ref()
            .and_then(|round| round.request.clone())
        else {
            return Ok(state);
        };
        let purpose = state
            .active_model_round
            .as_ref()
            .map(|round| round.purpose.clone())
            .ok_or("model_round_missing")?;
        let failure = attempt
            .failure
            .as_ref()
            .ok_or("model_attempt_failure_missing")?;
        let terminal = !failure.retryable || attempt.attempt_number >= request.maximum_attempts;
        if terminal {
            if attempt.attempt_number >= request.maximum_attempts {
                let identity = PreparedRequestIdentity {
                    activation_id: attempt.activation_id.clone(),
                    round_id: attempt.round_id.clone(),
                    request_id: attempt.request_id.clone(),
                    maximum_attempts: request.maximum_attempts,
                    attempt_id: attempt.attempt_id.clone(),
                    attempt_number: attempt.attempt_number,
                };
                let exhausted = append_model_attempts_exhausted(
                    self.store.clone(),
                    self.clock.clone(),
                    session_id.clone(),
                    &identity,
                    &attempt.attempt_id,
                    attempt.attempt_number,
                )
                .await?;
                self.observe_append(&exhausted.0, &exhausted.1).await;
                state = exhausted.1;
            }
            let (error_class, error_message) = terminal_model_error_class(&failure.error_class)?;
            return self
                .finish_model_execution_failure(
                    &session_id,
                    state,
                    purpose,
                    error_class,
                    error_message,
                )
                .await;
        }

        let abandoned = append_runtime_event(
            self.store.clone(),
            session_id,
            SessionEvent::ModelRequestAbandoned {
                activation_id: attempt.activation_id,
                round_id: attempt.round_id,
                request_id: attempt.request_id,
                attempt_id: attempt.attempt_id,
                reason: "runtime_restarted_after_retryable_failure".to_owned(),
                abandoned_at_ms: self.clock.now_ms(),
            },
        )
        .await?;
        self.observe_append(&abandoned.0, &abandoned.1).await;
        Ok(abandoned.1)
    }

    pub(super) async fn interrupt_unpaired_tool_calls(
        self: &Arc<Self>,
        session_id: String,
        state: VerifiedSessionState,
    ) -> Result<VerifiedSessionState, &'static str> {
        let calls = pending_tool_calls(&state);
        if calls.is_empty() {
            return Ok(state);
        }
        let append =
            append_interrupted_tool_results(self.store.clone(), session_id, state, calls).await?;
        self.observe_append(&append.0, &append.1).await;
        Ok(append.1)
    }

    async fn resolve_profile(
        &self,
        selection: &SessionSelection,
    ) -> Result<ProfileExecution, ModelError> {
        self.profiles
            .resolve(selection)
            .await
            .map_err(|error| match error {
                ProfileResolveError::InvalidSelection => ModelError::InvalidSelection,
                ProfileResolveError::NotFound | ProfileResolveError::AuthUnavailable => {
                    ModelError::ProfileUnavailable
                }
                ProfileResolveError::Backend => ModelError::Unavailable,
            })
    }

    async fn execute_prepared_model_request(
        self: &Arc<Self>,
        input: PreparedModelRequestInput<'_>,
    ) -> Result<PreparedModelExecution, &'static str> {
        let PreparedModelRequestInput {
            session_id,
            mut state,
            request,
            identity: request_identity,
            purpose,
        } = input;
        let request_id = request_identity.request_id.clone();
        let mut attempt_number = request_identity.attempt_number;
        let mut attempt_id = request_identity.attempt_id.clone();
        loop {
            if !state.mailbox.is_empty() {
                state = self
                    .abandon_model_request_for_mailbox(
                        session_id,
                        state,
                        request_identity,
                        &attempt_id,
                        attempt_number,
                        true,
                    )
                    .await?;
                return Ok(PreparedModelExecution {
                    state,
                    completion: PreparedModelCompletion::MailboxPending,
                });
            }
            let completion = match self.resolve_profile(&request.selection).await {
                Ok(execution) => self.model.complete(&request, execution).await,
                Err(error) => Err(error),
            };
            match completion {
                Ok(outcome) => {
                    return Ok(PreparedModelExecution {
                        state,
                        completion: PreparedModelCompletion::Completed {
                            outcome,
                            attempt_id,
                        },
                    });
                }
                Err(ModelError::ProfileUnavailable) => {
                    let failure = append_model_lifecycle_failure(
                        self.store.clone(),
                        session_id.to_owned(),
                        ModelFailureInput {
                            identity: request_identity,
                            attempt_id: &attempt_id,
                            attempt_number,
                            error_class: "profile_unavailable",
                            retryable: false,
                            provider_input: None,
                        },
                    )
                    .await?;
                    self.observe_append(&failure.0, &failure.1).await;
                    let mut current_state = failure.1;
                    if attempt_number >= request_identity.maximum_attempts {
                        let exhausted = append_model_attempts_exhausted(
                            self.store.clone(),
                            self.clock.clone(),
                            session_id.to_owned(),
                            request_identity,
                            &attempt_id,
                            attempt_number,
                        )
                        .await?;
                        self.observe_append(&exhausted.0, &exhausted.1).await;
                        current_state = exhausted.1;
                    }
                    let terminal = self
                        .finish_model_execution_failure(
                            session_id,
                            current_state,
                            purpose,
                            ModelAttemptErrorClass::ProfileUnavailable,
                            "profile unavailable",
                        )
                        .await?;
                    return Ok(PreparedModelExecution {
                        state: terminal,
                        completion: PreparedModelCompletion::Terminal,
                    });
                }
                Err(error) => {
                    if let ModelError::ProviderFailed(failure) = &error {
                        tracing::warn!(
                            session_id,
                            activation_id = %request_identity.activation_id,
                            round_id = %request_identity.round_id,
                            request_id = %request_identity.request_id,
                            attempt_id,
                            attempt_number,
                            stage = failure.stage,
                            retryable = failure.retryable,
                            status_code = ?failure.status_code,
                            provider_code = ?failure.provider_code,
                            provider_request_id = ?failure.request_id,
                            message = %failure.message,
                            "model provider attempt failed"
                        );
                    }
                    let error_class = model_error_class(&error);
                    let retryable_error = model_error_is_retryable(&error);
                    let provider_input = match &error {
                        ModelError::ProviderFailed(failure) => failure.provider_input.clone(),
                        _ => None,
                    };
                    let retryable =
                        retryable_error && attempt_number < request_identity.maximum_attempts;
                    let failed = append_model_lifecycle_failure(
                        self.store.clone(),
                        session_id.to_owned(),
                        ModelFailureInput {
                            identity: request_identity,
                            attempt_id: &attempt_id,
                            attempt_number,
                            error_class,
                            retryable,
                            provider_input,
                        },
                    )
                    .await?;
                    self.observe_append(&failed.0, &failed.1).await;
                    state = failed.1.clone();
                    if !retryable_error {
                        let (terminal_class, terminal_message) = terminal_model_error(&error);
                        let terminal = self
                            .finish_model_execution_failure(
                                session_id,
                                state,
                                purpose,
                                terminal_class,
                                terminal_message,
                            )
                            .await?;
                        return Ok(PreparedModelExecution {
                            state: terminal,
                            completion: PreparedModelCompletion::Terminal,
                        });
                    }
                    if attempt_number >= request_identity.maximum_attempts {
                        let exhausted = append_model_attempts_exhausted(
                            self.store.clone(),
                            self.clock.clone(),
                            session_id.to_owned(),
                            request_identity,
                            &attempt_id,
                            attempt_number,
                        )
                        .await?;
                        self.observe_append(&exhausted.0, &exhausted.1).await;
                        let (terminal_class, terminal_message) = terminal_model_error(&error);
                        let terminal = self
                            .finish_model_execution_failure(
                                session_id,
                                exhausted.1,
                                purpose,
                                terminal_class,
                                terminal_message,
                            )
                            .await?;
                        return Ok(PreparedModelExecution {
                            state: terminal,
                            completion: PreparedModelCompletion::Terminal,
                        });
                    }
                    if !state.mailbox.is_empty() {
                        state = self
                            .abandon_model_request_for_mailbox(
                                session_id,
                                state,
                                request_identity,
                                &attempt_id,
                                attempt_number,
                                false,
                            )
                            .await?;
                        return Ok(PreparedModelExecution {
                            state,
                            completion: PreparedModelCompletion::MailboxPending,
                        });
                    }
                    let next_number = attempt_number.saturating_add(1);
                    let delay = retry_delay_ms(
                        self.options.model_retry_base,
                        self.options.model_retry_max,
                        next_number,
                    );
                    let schedule = ModelRetrySchedule {
                        activation_id: request_identity.activation_id.clone(),
                        round_id: request_identity.round_id.clone(),
                        request_id: request_id.clone(),
                        failed_attempt_id: attempt_id.clone(),
                        failed_attempt_number: attempt_number,
                        next_attempt_number: next_number,
                        delay_ms: delay,
                        not_before_ms: self.clock.now_ms(),
                        maximum_attempts: request_identity.maximum_attempts,
                        error_class: error_class.to_owned(),
                    };
                    let scheduled = append_runtime_event_from_state(
                        self.store.clone(),
                        session_id.to_owned(),
                        state,
                        SessionEvent::ModelStepRetryScheduled { schedule },
                    )
                    .await?;
                    self.observe_append(&scheduled.0, &scheduled.1).await;
                    state = scheduled.1;
                    if !state.mailbox.is_empty() {
                        state = self
                            .abandon_model_request_for_mailbox(
                                session_id,
                                state,
                                request_identity,
                                &attempt_id,
                                attempt_number,
                                false,
                            )
                            .await?;
                        return Ok(PreparedModelExecution {
                            state,
                            completion: PreparedModelCompletion::MailboxPending,
                        });
                    }
                    if delay > 0 {
                        self.clock.sleep(Duration::from_millis(delay)).await;
                    }
                    let activation_id = request_identity.activation_id.clone();
                    let round_id = request_identity.round_id.clone();
                    let started_request_id = request_id.clone();
                    let started_at_ms = self.clock.now_ms();
                    let started = append_runtime_drafts_from_state(
                        self.store.clone(),
                        session_id.to_owned(),
                        state,
                        move |_| {
                            Ok(vec![EventDraft::identified(|attempt_id| {
                                SessionEvent::ModelAttemptStarted {
                                    activation_id: activation_id.clone(),
                                    round_id: round_id.clone(),
                                    request_id: started_request_id.clone(),
                                    attempt_id: attempt_id.to_owned(),
                                    attempt_number: next_number,
                                    started_at_ms,
                                }
                            })])
                        },
                    )
                    .await?;
                    self.observe_append(&started.0, &started.1).await;
                    state = started.1;
                    attempt_number = next_number;
                    attempt_id = state
                        .active_model_round
                        .as_ref()
                        .and_then(|round| round.attempt.as_ref())
                        .map(|attempt| attempt.attempt_id.clone())
                        .ok_or("model_retry_attempt_missing")?;
                }
            }
        }
    }

    async fn finish_model_execution_failure(
        self: &Arc<Self>,
        session_id: &str,
        mut state: VerifiedSessionState,
        purpose: ModelRequestPurpose,
        error_class: ModelAttemptErrorClass,
        error_message: &'static str,
    ) -> Result<VerifiedSessionState, &'static str> {
        match purpose {
            ModelRequestPurpose::Conversation => {
                if state
                    .transcript
                    .last()
                    .is_some_and(|message| message.role.is_mailbox_input())
                {
                    let terminal = append_model_attempt_failure_with_error(
                        self.store.clone(),
                        session_id.to_owned(),
                        state
                            .transcript
                            .last()
                            .map(|message| message.message_id.clone())
                            .unwrap_or_default(),
                        error_class,
                        error_message,
                    )
                    .await?;
                    self.observe_append(&terminal.0, &terminal.1).await;
                    state = terminal.1;
                }
            }
            ModelRequestPurpose::ContextHandoff => {
                let failed = append_context_handoff_failure(
                    self.store.clone(),
                    self.clock.clone(),
                    session_id.to_owned(),
                    &state,
                    error_message,
                    None,
                )
                .await?;
                self.observe_append(&failed.0, &failed.1).await;
                return Ok(failed.1);
            }
        }
        self.finish_model_failure_activation(session_id, state)
            .await
    }

    fn provider_tools(
        &self,
        state: &VerifiedSessionState,
    ) -> Result<Arc<Vec<ToolDefinition>>, &'static str> {
        let mut tools = self
            .tools
            .definitions(&self.definition.tools)
            .map_err(|_| "tool_selection")?;
        tools.extend(provider_runtime_tool_definitions(state));
        Ok(Arc::new(tools))
    }

    fn prepare_conversation_context(
        &self,
        state: &VerifiedSessionState,
        selection: &SessionSelection,
        cache: &mut ProviderContextCache,
    ) -> Result<PreparedConversationContext, &'static str> {
        let tools = self.provider_tools(state)?;
        let transcript = cache.prepare(
            state,
            state.system_prompt.as_deref().unwrap_or(""),
            |handoff| self.load_context_handoff_document(state, handoff),
        )?;
        let metrics = model_context_metrics(&transcript, &tools)?;
        let selection_fingerprint = model_selection_fingerprint(selection)?;
        let estimated_input_tokens = estimated_model_input_tokens_from_metrics(
            state,
            &transcript,
            &selection_fingerprint,
            &metrics.tool_schema_fingerprint,
            metrics.visible_input_estimate_tokens,
            &[],
        )?;
        Ok(PreparedConversationContext {
            transcript,
            tools,
            estimated_input_tokens,
            selection_fingerprint,
            prompt_fingerprint: metrics.prompt_fingerprint,
            tool_schema_fingerprint: metrics.tool_schema_fingerprint,
        })
    }

    fn load_context_handoff_document(
        &self,
        state: &VerifiedSessionState,
        handoff: &ContextHandoffState,
    ) -> Result<ContextHandoffDocument, &'static str> {
        let record = self
            .store
            .read_event(&state.session_id, &handoff.handoff_id)
            .map_err(|_| "context_handoff_read")?
            .ok_or("context_handoff_event")?;
        let SessionEvent::ContextHandoffCreated { handoff: document } = record.event else {
            return Err("context_handoff_event");
        };
        if !handoff.matches_document(&document) {
            return Err("context_handoff_event");
        }
        Ok(document)
    }

    pub(super) async fn ensure_model_context(
        self: &Arc<Self>,
        session_id: &str,
        selection: &SessionSelection,
        mut state: VerifiedSessionState,
        cache: &mut ProviderContextCache,
    ) -> Result<(VerifiedSessionState, Option<PreparedConversationContext>), String> {
        let Some(limits) = self
            .profiles
            .model_limits(selection)
            .map_err(|_| "profile_model")?
        else {
            let prepared = self.prepare_conversation_context(&state, selection, cache)?;
            return Ok((state, Some(prepared)));
        };
        let Some(normal_input_budget) = model_input_budget(&limits, limits.max_output_tokens)
        else {
            let state = self
                .finish_unhandoffable_model_context(session_id, state)
                .await?;
            return Ok((state, None));
        };
        loop {
            if state.active_activation.is_none() {
                return Ok((state, None));
            }
            if state.pending_context_handoff.is_some() {
                state = self
                    .run_context_handoff(session_id, selection, &state, cache)
                    .await?;
                if !state.mailbox.is_empty() || state.active_wait.is_some() {
                    return Ok((state, None));
                }
                continue;
            }

            let prepared = self.prepare_conversation_context(&state, selection, cache)?;
            if prepared.estimated_input_tokens <= normal_input_budget {
                return Ok((state, Some(prepared)));
            }

            let Some(mut plan) =
                build_context_handoff_plan(&state, selection, limits.max_output_tokens)?
            else {
                let state = self
                    .finish_unhandoffable_model_context(session_id, state)
                    .await?;
                return Ok((state, None));
            };
            let transcript = context_handoff_source(
                &state,
                &plan.covered_through_message_id,
                &prepared.transcript,
            )?;
            let provider_only_tail_start = prepared.transcript.len();
            let metrics = model_context_metrics(&transcript, &prepared.tools)?;
            let handoff_input_tokens = estimated_model_input_tokens_from_metrics(
                &state,
                &transcript,
                &prepared.selection_fingerprint,
                &metrics.tool_schema_fingerprint,
                metrics.visible_input_estimate_tokens,
                &transcript[provider_only_tail_start..],
            )?;
            let Some(available_output_tokens) = limits
                .context_window_tokens
                .checked_sub(handoff_input_tokens)
                .filter(|available| *available > 0)
            else {
                let state = self
                    .finish_unhandoffable_model_context(session_id, state)
                    .await?;
                return Ok((state, None));
            };
            plan.max_output_tokens = limits
                .max_output_tokens
                .min(u32::try_from(available_output_tokens).unwrap_or(u32::MAX));
            let request = UnboundModelRequest {
                selection: selection.clone(),
                transcript: Arc::new(transcript),
                tools: prepared.tools,
                prompt_fingerprint: metrics.prompt_fingerprint,
                tool_schema_fingerprint: metrics.tool_schema_fingerprint,
                max_output_tokens: Some(plan.max_output_tokens),
                stream_observer: Arc::new(SilentModelStreamObserver),
            };
            let planned = append_context_handoff_plan(
                self.store.clone(),
                self.clock.clone(),
                session_id.to_owned(),
                ContextHandoffPlanInput {
                    state: &state,
                    plan,
                    request: &request,
                    maximum_attempts: self.options.model_step_max_attempts,
                },
            )
            .await?;
            self.observe_append(&planned.0, &planned.1).await;
            state = planned.1;
        }
    }

    async fn run_context_handoff(
        self: &Arc<Self>,
        session_id: &str,
        selection: &SessionSelection,
        state: &VerifiedSessionState,
        cache: &mut ProviderContextCache,
    ) -> Result<VerifiedSessionState, String> {
        let plan = state
            .pending_context_handoff
            .clone()
            .ok_or("context_handoff_plan_missing")?;
        if &plan.selection != selection {
            return Err("context_handoff_selection_changed".to_owned());
        }
        let prepared = self.prepare_conversation_context(state, selection, cache)?;
        let transcript = context_handoff_source(
            state,
            &plan.covered_through_message_id,
            &prepared.transcript,
        )?;
        let metrics = model_context_metrics(&transcript, &prepared.tools)?;
        let handoff_tool_schema_fingerprint = metrics.tool_schema_fingerprint.clone();
        let request = UnboundModelRequest {
            selection: selection.clone(),
            transcript: Arc::new(transcript),
            tools: prepared.tools,
            prompt_fingerprint: metrics.prompt_fingerprint,
            tool_schema_fingerprint: metrics.tool_schema_fingerprint,
            max_output_tokens: Some(plan.max_output_tokens),
            stream_observer: Arc::new(SilentModelStreamObserver),
        };
        let (preparation_appends, prepared_state, request_identity) = prepare_model_round(
            self.store.clone(),
            self.clock.clone(),
            session_id.to_owned(),
            ModelRoundInput {
                state,
                selection,
                request: &request,
                purpose: ModelRequestPurpose::ContextHandoff,
                maximum_attempts: self.options.model_step_max_attempts,
            },
        )
        .await?;
        for (append, state_after_append) in &preparation_appends {
            self.observe_append(append, state_after_append).await;
        }
        drop(preparation_appends);
        let request = request.bind(
            session_id,
            request_identity.activation_id.clone(),
            request_identity.round_id.clone(),
        );
        let execution = self
            .execute_prepared_model_request(PreparedModelRequestInput {
                session_id,
                state: prepared_state,
                request,
                identity: &request_identity,
                purpose: ModelRequestPurpose::ContextHandoff,
            })
            .await?;
        let (outcome, attempt_id) = match execution.completion {
            PreparedModelCompletion::Completed {
                outcome,
                attempt_id,
            } => (outcome, attempt_id),
            PreparedModelCompletion::MailboxPending | PreparedModelCompletion::Terminal => {
                return Ok(execution.state)
            }
        };
        let handoff_selection_fingerprint = model_selection_fingerprint(selection)?;
        let usage = outcome.usage.as_ref().map(|usage| ModelUsageAnchor {
            context_generation: model_context_generation(state),
            selection_fingerprint: handoff_selection_fingerprint,
            tool_schema_fingerprint: handoff_tool_schema_fingerprint,
            result_event_id: None,
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
            output_reasoning_tokens: usage.output_reasoning_tokens,
            output_text_tokens: usage.output_text_tokens,
        });
        let document = match outcome.tool_calls.as_slice() {
            [call] if call.tool_name == CONTEXT_HANDOFF_TOOL_NAME => {
                context_handoff_document(&call.arguments).ok()
            }
            _ => None,
        };
        let Some(document) = document else {
            let rejected = append_context_handoff_rejection(
                self.store.clone(),
                session_id.to_owned(),
                ContextHandoffRejectionInput {
                    identity: request_identity,
                    attempt_id,
                    plan_id: plan.plan_id,
                    usage,
                    assistant_content: outcome.text,
                    tool_calls: outcome.tool_calls,
                    provider_context: outcome.provider_context,
                    provider_input: outcome.provider_input.map(|input| *input),
                },
            )
            .await?;
            self.observe_append(&rejected.0, &rejected.1).await;
            return Ok(rejected.1);
        };
        let document_tokens = outcome
            .usage
            .as_ref()
            .and_then(|usage| usage.output_text_tokens);
        let handoff = ContextHandoffDocumentDraft {
            plan_id: plan.plan_id.clone(),
            previous_handoff_id: plan.previous_handoff_id.clone(),
            next_generation: plan.next_generation,
            covered_through_message_id: plan.covered_through_message_id.clone(),
            document,
            document_tokens,
            selection: plan.selection.clone(),
        };
        let completed = append_context_handoff_document(
            self.store.clone(),
            session_id.to_owned(),
            &request_identity,
            &attempt_id,
            handoff,
            usage,
            outcome.provider_input.map(|input| *input),
        )
        .await?;
        self.observe_append(&completed.0, &completed.1).await;
        Ok(completed.1)
    }

    async fn finish_unhandoffable_model_context(
        self: &Arc<Self>,
        session_id: &str,
        mut state: VerifiedSessionState,
    ) -> Result<VerifiedSessionState, &'static str> {
        let trigger_message_id = state
            .transcript
            .iter()
            .rev()
            .find(|message| message.role.is_mailbox_input())
            .map(|message| message.message_id.clone())
            .ok_or("model_context_trigger_missing")?;
        let failed = append_model_attempt_failure_with_error(
            self.store.clone(),
            session_id.to_owned(),
            trigger_message_id,
            ModelAttemptErrorClass::ContextHandoffFailed,
            "model context exceeds its input budget and has no durable handoff boundary",
        )
        .await?;
        self.observe_append(&failed.0, &failed.1).await;
        state = failed.1;
        self.finish_model_failure_activation(session_id, state)
            .await
    }

    pub(super) async fn run_model_round(
        self: &Arc<Self>,
        session_id: &str,
        selection: &SessionSelection,
        state: &VerifiedSessionState,
        prepared: PreparedConversationContext,
    ) -> Result<
        (
            Vec<(AppendResult, VerifiedSessionState)>,
            VerifiedSessionState,
        ),
        String,
    > {
        let PreparedConversationContext {
            transcript,
            tools,
            estimated_input_tokens: _,
            selection_fingerprint,
            prompt_fingerprint,
            tool_schema_fingerprint,
        } = prepared;
        let limits = self
            .profiles
            .model_limits(selection)
            .map_err(|_| "profile_model")?;
        let request = UnboundModelRequest {
            selection: selection.clone(),
            transcript,
            tools: tools.clone(),
            prompt_fingerprint,
            tool_schema_fingerprint: tool_schema_fingerprint.clone(),
            max_output_tokens: limits.as_ref().map(|limits| limits.max_output_tokens),
            stream_observer: self.stream_observer.clone(),
        };
        let (preparation_appends, prepared_state, request_identity) = prepare_model_round(
            self.store.clone(),
            self.clock.clone(),
            session_id.to_owned(),
            ModelRoundInput {
                state,
                selection,
                request: &request,
                purpose: ModelRequestPurpose::Conversation,
                maximum_attempts: self.options.model_step_max_attempts,
            },
        )
        .await?;
        for (append, state_after_append) in &preparation_appends {
            self.observe_append(append, state_after_append).await;
        }
        drop(preparation_appends);
        // The ModelRoundStarted Event ULID is the durable round identity. Use
        // it for transient provider observations after the boundary commits.
        let request = request.bind(
            session_id,
            request_identity.activation_id.clone(),
            request_identity.round_id.clone(),
        );
        let execution = self
            .execute_prepared_model_request(PreparedModelRequestInput {
                session_id,
                state: prepared_state,
                request,
                identity: &request_identity,
                purpose: ModelRequestPurpose::Conversation,
            })
            .await?;
        let (outcome, attempt_id) = match execution.completion {
            PreparedModelCompletion::Completed {
                outcome,
                attempt_id,
            } => (outcome, attempt_id),
            PreparedModelCompletion::MailboxPending | PreparedModelCompletion::Terminal => {
                return Ok((Vec::new(), execution.state))
            }
        };
        let completed_state = execution.state;
        let usage = match outcome.usage.as_ref() {
            Some(usage) => {
                let context_generation = model_context_generation(state);
                Some(ModelUsageAnchor {
                    context_generation,
                    selection_fingerprint,
                    tool_schema_fingerprint,
                    // Filled with the MessageAppended Event ULID by the atomic
                    // model-result batch.
                    result_event_id: None,
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    output_reasoning_tokens: usage.output_reasoning_tokens,
                    output_text_tokens: usage.output_text_tokens,
                })
            }
            None => None,
        };
        let has_tool_calls = !outcome.tool_calls.is_empty();
        let completed = append_model_result(
            self.store.clone(),
            session_id.to_owned(),
            completed_state,
            ModelResultInput {
                identity: request_identity,
                attempt_id,
                usage,
                assistant_content: outcome.text,
                provider_context: outcome.provider_context,
                tool_calls: outcome.tool_calls,
                provider_input: outcome.provider_input.map(|input| *input),
            },
        )
        .await?;
        self.observe_append(&completed.0, &completed.1).await;
        if !has_tool_calls {
            return Ok((Vec::new(), completed.1));
        }
        let state = self
            .execute_pending_tool_calls(session_id, completed.1)
            .await?;
        Ok((Vec::new(), state))
    }
}

pub(super) fn model_error_is_retryable(error: &ModelError) -> bool {
    match error {
        ModelError::Unavailable => true,
        ModelError::ProviderFailed(failure) => failure.retryable,
        ModelError::InvalidSelection
        | ModelError::ProfileUnavailable
        | ModelError::InvalidToolArguments => false,
    }
}
