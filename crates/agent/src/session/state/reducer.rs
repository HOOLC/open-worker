use super::*;
use std::collections::{HashMap, HashSet};

impl SessionState {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            created_at_ms: None,
            selection: SessionSelection::default(),
            system_prompt: None,
            workspace: String::new(),
            transcript: Vec::new(),
            last_model_attempt_failure: None,
            mailbox: Vec::new(),
            consumed_through_mailbox_seq: 0,
            active_wait: None,
            active_timer: None,
            wake_pending_wait_id: None,
            inflight_tool_call_ids: BTreeSet::new(),
            active_activation: None,
            last_activation_outcome: None,
            active_model_round: None,
            pending_context_handoff: None,
            latest_context_handoff: None,
            latest_model_usage: None,
            last_context_handoff_failure: None,
            last_model_attempts_exhausted: None,
            stream_version: 0,
        }
    }

    pub fn apply_event(&self, event: &SessionEvent) -> Result<Self, DomainError> {
        self.validate()?;
        let next = self.apply_event_from_valid_state(event)?;
        next.validate()?;
        Ok(next)
    }

    /// Apply one event to a projection whose complete invariants were already
    /// verified by the caller.
    ///
    /// Storage uses this only while carrying its opaque verified projection
    /// through one event batch. Public reducer entry points continue to
    /// validate both the input and the resulting state. Event-local checks stay
    /// here so a verified projection cannot admit an invalid transition merely
    /// because its unchanged history was not scanned again.
    pub(crate) fn apply_event_from_valid_state(
        &self,
        event: &SessionEvent,
    ) -> Result<Self, DomainError> {
        self.clone().apply_event_from_valid_state_owned(event)
    }

    pub(crate) fn apply_event_from_valid_state_owned(
        mut self,
        event: &SessionEvent,
    ) -> Result<Self, DomainError> {
        self.validate_event_position(event)?;
        event.validate()?;
        if !self.apply_payload(event)? {
            return Ok(self);
        }
        // Validate the projected state at the version that the event will
        // occupy. Creation installs timestamp/selection while the
        // input projection still has stream_version zero; validating before
        // advancing would classify that legitimate transition as an
        // uncreated state. No-op transitions return above and retain the
        // caller's version for reducer-level idempotency filtering.
        self.stream_version = self
            .stream_version
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        Ok(self)
    }

    pub fn decide_batch(&self, events: &[SessionEvent]) -> Result<DomainDecision, DomainError> {
        if events.is_empty() {
            return Err(DomainError::EmptyEventBatch);
        }
        self.validate()?;
        let mut state = self.clone();
        let mut effective_events = Vec::with_capacity(events.len());
        for event in events {
            let next = state.apply_event_from_valid_state(event)?;
            if next.stream_version != state.stream_version {
                effective_events.push(event.clone());
            }
            state = next;
        }
        state.validate()?;
        Ok(DomainDecision {
            effective_events,
            state,
        })
    }

    pub fn apply_events<I>(&self, events: I) -> Result<Self, DomainError>
    where
        I: IntoIterator<Item = SessionEvent>,
    {
        self.validate()?;
        let next = events.into_iter().try_fold(self.clone(), |state, event| {
            state.apply_event_from_valid_state_owned(&event)
        })?;
        next.validate()?;
        Ok(next)
    }

    pub fn apply_record(&self, record: &EventRecord) -> Result<Self, DomainError> {
        self.validate()?;
        let next = self.clone().apply_record_from_valid_state_owned(record)?;
        next.validate()?;
        Ok(next)
    }

    pub(crate) fn apply_record_from_valid_state_owned(
        mut self,
        record: &EventRecord,
    ) -> Result<Self, DomainError> {
        if record.event_schema_version != EVENT_SCHEMA_VERSION {
            return Err(DomainError::UnsupportedEventSchema(
                record.event_schema_version,
            ));
        }
        if record.stream_id != self.session_id {
            return Err(DomainError::SessionMismatch {
                expected: self.session_id.clone(),
                actual: record.stream_id.clone(),
            });
        }
        let expected_version = self
            .stream_version
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        if record.stream_version != expected_version {
            return Err(DomainError::StreamVersionGap {
                expected: expected_version,
                actual: record.stream_version,
            });
        }
        self.validate_event_position(&record.event)?;
        record.event.validate()?;
        self.apply_payload(&record.event)?;
        self.stream_version = record.stream_version;
        Ok(self)
    }

    /// Rebuild a projection from the immutable event records in stream order.
    ///
    /// This is intentionally the only replay path exposed by the domain.  It
    /// does not inspect storage metadata or allocate repair facts; callers
    /// provide the records and receive either a complete projection or the
    /// first invalid transition.
    pub fn replay<I>(session_id: impl Into<String>, records: I) -> Result<Self, DomainError>
    where
        I: IntoIterator<Item = EventRecord>,
    {
        let mut state = Self::new(session_id);
        let mut records = records.into_iter().peekable();
        while let Some(first) = records.next() {
            if first.batch_index != 0 || first.batch_size == 0 {
                return Err(DomainError::InvalidState(
                    "event batch does not start at index zero".into(),
                ));
            }
            let batch_size = first.batch_size;
            let mut batch = vec![first];
            while records.peek().is_some_and(|record| record.batch_index != 0) {
                batch.push(records.next().expect("peeked event record"));
            }
            for (index, record) in batch.iter().enumerate() {
                if record.batch_size != batch_size
                    || usize::try_from(record.batch_index).ok() != Some(index)
                {
                    return Err(DomainError::InvalidState(
                        "event batch metadata is not contiguous".into(),
                    ));
                }
            }
            let expected = usize::try_from(batch_size).map_err(|_| {
                DomainError::InvalidState("event batch size exceeds this platform".into())
            })?;
            let complete_domain_batch = batch.len() == expected;
            let handoff_batch_without_storage_snapshot = batch.len().checked_add(1)
                == Some(expected)
                && matches!(
                    batch.first().map(|record| &record.event),
                    Some(SessionEvent::ContextHandoffCreated { .. })
                )
                && matches!(
                    batch.last().map(|record| &record.event),
                    Some(SessionEvent::ModelRequestCompleted { .. })
                );
            if !complete_domain_batch && !handoff_batch_without_storage_snapshot {
                return Err(DomainError::InvalidState(
                    "event batch is incomplete".into(),
                ));
            }
            for record in batch {
                state = state.apply_record_from_valid_state_owned(&record)?;
            }
            state.validate()?;
        }
        Ok(state)
    }

    pub fn terminal_model_failure_for_last_input(&self) -> Option<&ModelAttemptFailure> {
        let message = self.transcript.last()?;
        let failure = self.last_model_attempt_failure.as_ref()?;
        (message.role.is_mailbox_input() && message.message_id == failure.trigger_message_id)
            .then_some(failure)
    }

    pub fn unresolved_input(&self) -> Option<&TranscriptMessage> {
        let trigger = self.transcript.last()?;
        if trigger.role.is_mailbox_input() && self.terminal_model_failure_for_last_input().is_none()
        {
            Some(trigger)
        } else {
            None
        }
    }

    /// Public session status is inferred from the projection. Working/idle
    /// are not stored facts.
    pub fn work_status(&self) -> &'static str {
        if self.active_activation.is_some()
            || self.active_model_round.is_some()
            || self.active_wait.is_some()
            || self.has_inflight_tool_effect()
            || !self.mailbox.is_empty()
            || self.unresolved_input().is_some()
            || self.model_followup_identity().is_some()
        {
            "working"
        } else {
            "idle"
        }
    }

    /// Startup and wake only schedule execution when durable work can make
    /// progress.
    pub fn is_startup_runnable(&self) -> bool {
        if self.has_inflight_tool_effect() {
            return false;
        }
        if !self.mailbox.is_empty() {
            return true;
        }
        if self.last_context_handoff_failure.is_some() {
            return false;
        }
        if self.active_wait.is_some() {
            return false;
        }
        self.unresolved_input().is_some() || self.model_followup_identity().is_some()
    }

    pub fn has_inflight_tool_effect(&self) -> bool {
        !self.inflight_tool_call_ids.is_empty()
    }

    pub fn model_followup_identity(&self) -> Option<String> {
        if self.active_wait.is_some() {
            return None;
        }
        if self.active_activation.is_none()
            && self.last_activation_outcome.as_ref() != Some(&ActivationOutcome::Wait)
        {
            return None;
        }
        let latest = self.transcript.last()?;
        if latest.role == TranscriptRole::Assistant && latest.tool_calls.is_empty() {
            return Some(latest.message_id.clone());
        }
        if latest.role != TranscriptRole::Tool {
            return None;
        }
        let assistant = self
            .transcript
            .iter()
            .rev()
            .skip_while(|message| message.role == TranscriptRole::Tool)
            .find(|message| {
                message.role == TranscriptRole::Assistant && !message.tool_calls.is_empty()
            })?;

        let all_tools_terminal = assistant
            .tool_calls
            .iter()
            .all(|call| !self.inflight_tool_call_ids.contains(&call.tool_call_id));
        all_tools_terminal.then(|| assistant.message_id.clone())
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        require_text("session_id", &self.session_id)?;
        match (self.stream_version, self.created_at_ms) {
            (0, None) => {
                if self.selection != SessionSelection::default()
                    || self.system_prompt.is_some()
                    || !self.workspace.is_empty()
                {
                    return Err(DomainError::InvalidState(
                        "uncreated session contains creation facts".into(),
                    ));
                }
            }
            (0, _) => {
                return Err(DomainError::InvalidState(
                    "uncreated session contains creation facts".into(),
                ));
            }
            (_, Some(created_at_ms)) => {
                if created_at_ms < 0 {
                    return Err(DomainError::InvalidCreatedAt);
                }
                self.selection.validate()?;
                validate_workspace(&self.workspace)?;
            }
            _ => return Err(DomainError::SessionNotCreated),
        }
        let mut message_ids = HashSet::with_capacity(self.transcript.len());
        let mut input_message_ids = HashSet::new();
        let mut declared_tool_calls = HashMap::new();
        let mut computed_inflight = BTreeSet::new();
        for message in &self.transcript {
            validate_message(message)?;
            if !message_ids.insert(message.message_id.as_str()) {
                return Err(DomainError::ConflictingTranscriptMessage(
                    message.message_id.clone(),
                ));
            }
            if message.role.is_mailbox_input() {
                input_message_ids.insert(message.message_id.as_str());
            }
            if !message.tool_calls.is_empty() && message.role != TranscriptRole::Assistant {
                return Err(DomainError::InvalidState(
                    "only assistant messages may declare tool calls".into(),
                ));
            }
            if message.is_error && message.role != TranscriptRole::Tool {
                return Err(DomainError::InvalidState(
                    "is_error may only be set on tool messages".into(),
                ));
            }
            if message.role == TranscriptRole::Tool && message.tool_call_id.is_none() {
                return Err(DomainError::InvalidState(
                    "tool messages require tool_call_id".into(),
                ));
            }
            if !message.tool_calls.is_empty() && !computed_inflight.is_empty() {
                return Err(DomainError::InvalidState(
                    "a second tool batch cannot start before the current batch finishes".into(),
                ));
            }
            for call in &message.tool_calls {
                if declared_tool_calls
                    .insert(call.tool_call_id.as_str(), call)
                    .is_some()
                {
                    return Err(DomainError::DuplicateTranscriptToolCallId(
                        call.tool_call_id.clone(),
                    ));
                }
                computed_inflight.insert(call.tool_call_id.clone());
            }
            if let Some(tool_call_id) = &message.tool_call_id {
                if message.role != TranscriptRole::Tool {
                    return Err(DomainError::InvalidState(
                        "tool_call_id may only be attached to tool messages".into(),
                    ));
                }
                if !declared_tool_calls.contains_key(tool_call_id.as_str()) {
                    return Err(DomainError::UnknownToolCall(tool_call_id.clone()));
                }
                if !computed_inflight.remove(tool_call_id) {
                    return Err(DomainError::ToolCallAlreadyFinished(tool_call_id.clone()));
                }
            }
        }
        if computed_inflight != self.inflight_tool_call_ids {
            return Err(DomainError::InvalidState(
                "inflight tool call cache does not match transcript".into(),
            ));
        }
        if let Some(failure) = &self.last_model_attempt_failure {
            validate_model_attempt_failure(failure)?;
            if !input_message_ids.contains(failure.trigger_message_id.as_str()) {
                return Err(DomainError::InvalidState(
                    "model attempt failure has no causal input message".into(),
                ));
            }
        }
        let mut expected_mailbox_seq = self
            .consumed_through_mailbox_seq
            .checked_add(1)
            .ok_or(DomainError::VersionOverflow)?;
        let mut mailbox_message_ids = HashSet::with_capacity(self.mailbox.len());
        for message in &self.mailbox {
            validate_mailbox_message(message)?;
            if message.mailbox_seq != expected_mailbox_seq {
                return Err(DomainError::MailboxOrder {
                    expected: expected_mailbox_seq,
                    actual: message.mailbox_seq,
                });
            }
            if !mailbox_message_ids.insert(message.message_id.as_str())
                || message_ids.contains(message.message_id.as_str())
            {
                return Err(DomainError::ConflictingMailboxMessage(
                    message.message_id.clone(),
                ));
            }
            expected_mailbox_seq = expected_mailbox_seq
                .checked_add(1)
                .ok_or(DomainError::VersionOverflow)?;
        }
        if let Some(wait) = &self.active_wait {
            validate_wait(wait)?;
            if let Some(pending_wait_id) = &self.wake_pending_wait_id {
                if pending_wait_id != &wait.wait_id {
                    return Err(DomainError::InvalidState(
                        "pending wake belongs to a different active wait".into(),
                    ));
                }
            }
        } else if self.wake_pending_wait_id.is_some() {
            return Err(DomainError::InvalidState(
                "pending wake requires an active wait".into(),
            ));
        }
        if let Some(timer) = &self.active_timer {
            if self
                .active_wait
                .as_ref()
                .is_none_or(|wait| wait.wait_id != timer.wait_id)
            {
                return Err(DomainError::InvalidState(
                    "wait timer must belong to the active wait".into(),
                ));
            }
            if timer.deadline_ms
                != self
                    .active_wait
                    .as_ref()
                    .map(|wait| wait.deadline_ms)
                    .unwrap_or_default()
            {
                return Err(DomainError::InvalidState(
                    "wait timer deadline does not match active wait".into(),
                ));
            }
        }
        if let Some(activation) = &self.active_activation {
            validate_active_activation(activation)?;
            if self.last_activation_outcome.is_some() {
                return Err(DomainError::InvalidState(
                    "active activation cannot also have a terminal outcome".into(),
                ));
            }
        }
        if let Some(round) = &self.active_model_round {
            validate_active_model_round(round)?;
            let Some(activation) = &self.active_activation else {
                return Err(DomainError::InvalidState(
                    "active model round requires an active activation".into(),
                ));
            };
            if round.activation_id != activation.activation_id {
                return Err(DomainError::InvalidState(
                    "active model round belongs to another activation".into(),
                ));
            }
        }
        if let Some(plan) = &self.pending_context_handoff {
            validate_context_handoff_plan(plan)?;
            let Some(activation) = &self.active_activation else {
                return Err(DomainError::InvalidState(
                    "pending context handoff requires an active activation".into(),
                ));
            };
            if plan.activation_id != activation.activation_id
                || activation.selection != plan.selection
            {
                return Err(DomainError::InvalidState(
                    "pending context handoff belongs to another activation".into(),
                ));
            }
            if plan.previous_handoff_id.as_deref()
                != self
                    .latest_context_handoff
                    .as_ref()
                    .map(|handoff| handoff.handoff_id.as_str())
            {
                return Err(DomainError::InvalidState(
                    "pending context handoff has a stale parent".into(),
                ));
            }
            let expected_generation = self
                .latest_context_handoff
                .as_ref()
                .map_or(2, |handoff| handoff.next_generation.saturating_add(1));
            if plan.next_generation != expected_generation {
                return Err(DomainError::InvalidState(
                    "pending context handoff has an invalid generation".into(),
                ));
            }
            if self
                .transcript
                .last()
                .map(|message| message.message_id.as_str())
                != Some(plan.covered_through_message_id.as_str())
            {
                return Err(DomainError::InvalidState(
                    "pending context handoff boundary is not the latest live message".into(),
                ));
            }
        }
        if let Some(handoff) = &self.latest_context_handoff {
            validate_context_handoff_state(handoff)?;
        }
        if let Some(usage) = &self.latest_model_usage {
            validate_model_usage_anchor(usage)?;
        }
        if let Some(error) = &self.last_context_handoff_failure {
            validate_model_error(error)?;
        }
        if let Some(round) = &self.active_model_round {
            match round.purpose {
                ModelRequestPurpose::Conversation => {
                    let completed = round
                        .attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.outcome == ModelAttemptOutcome::Completed);
                    if self.pending_context_handoff.is_some() && !completed {
                        return Err(DomainError::InvalidState(
                            "pending context handoff requires a handoff round".into(),
                        ));
                    }
                }
                ModelRequestPurpose::ContextHandoff => {
                    let completed = round
                        .attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.outcome == ModelAttemptOutcome::Completed);
                    if self.pending_context_handoff.is_none()
                        && !completed
                        && self.last_context_handoff_failure.is_none()
                    {
                        return Err(DomainError::InvalidState(
                            "active context handoff round has no durable plan".into(),
                        ));
                    }
                }
            }
        }
        if let Some(fact) = &self.last_model_attempts_exhausted {
            validate_model_attempts_exhausted(fact)?;
        }
        Ok(())
    }

    fn apply_payload(&mut self, event: &SessionEvent) -> Result<bool, DomainError> {
        match event {
            SessionEvent::SessionCreated {
                session_id,
                created_at_ms,
                selection,
                system_prompt,
                workspace,
                ..
            } => {
                if session_id != &self.session_id {
                    return Err(DomainError::SessionMismatch {
                        expected: self.session_id.clone(),
                        actual: session_id.clone(),
                    });
                }
                self.created_at_ms = Some(*created_at_ms);
                self.selection = selection.clone();
                self.system_prompt = system_prompt.clone();
                self.workspace = workspace.clone();
            }
            SessionEvent::SelectionChanged { selection } => {
                if self.active_activation.is_some()
                    || self.active_model_round.is_some()
                    || self.active_wait.is_some()
                    || self.has_inflight_tool_effect()
                {
                    return Err(DomainError::InvalidState(
                        "selection cannot change while the session has an active effect".into(),
                    ));
                }
                if self.selection == *selection {
                    return Ok(false);
                }
                self.selection = selection.clone();
                self.last_model_attempt_failure = None;
                self.last_model_attempts_exhausted = None;
                self.last_context_handoff_failure = None;
                self.pending_context_handoff = None;
                self.latest_model_usage = None;
            }
            SessionEvent::MailboxMessageAppended { message } => {
                if self
                    .mailbox
                    .iter()
                    .any(|existing| existing.message_id == message.message_id)
                    || self
                        .transcript
                        .iter()
                        .any(|existing| existing.message_id == message.message_id)
                {
                    return Ok(false);
                }
                let expected = self
                    .consumed_through_mailbox_seq
                    .checked_add(self.mailbox.len() as u64 + 1)
                    .ok_or(DomainError::VersionOverflow)?;
                if message.mailbox_seq != expected {
                    return Err(DomainError::MailboxOrder {
                        expected,
                        actual: message.mailbox_seq,
                    });
                }
                self.mailbox.push(message.clone());
                if let Some(wait) = &self.active_wait {
                    self.wake_pending_wait_id = Some(wait.wait_id.clone());
                }
            }
            SessionEvent::MailboxDrained {
                through_mailbox_seq,
            } => {
                if *through_mailbox_seq <= self.consumed_through_mailbox_seq {
                    return Ok(false);
                }
                let available_through = self
                    .mailbox
                    .last()
                    .map(|message| message.mailbox_seq)
                    .ok_or(DomainError::MailboxDrainBeyondAvailable {
                        requested: *through_mailbox_seq,
                        available: self.consumed_through_mailbox_seq,
                    })?;
                if *through_mailbox_seq > available_through {
                    return Err(DomainError::MailboxDrainBeyondAvailable {
                        requested: *through_mailbox_seq,
                        available: available_through,
                    });
                }
                let drain_count = self
                    .mailbox
                    .iter()
                    .take_while(|message| message.mailbox_seq <= *through_mailbox_seq)
                    .count();
                for message in self.mailbox.drain(..drain_count) {
                    self.transcript.push(TranscriptMessage {
                        message_id: message.message_id,
                        role: TranscriptRole::User,
                        content: message.content,
                        is_error: false,
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                        provider_context: None,
                        source_mailbox_seq: Some(message.mailbox_seq),
                    });
                }
                self.consumed_through_mailbox_seq = *through_mailbox_seq;
                self.active_wait = None;
                self.active_timer = None;
                self.wake_pending_wait_id = None;
                self.pending_context_handoff = None;
            }
            SessionEvent::MessageAppended { message, wake_wait } => {
                if let Some(existing) = self
                    .transcript
                    .iter()
                    .find(|existing| existing.message_id == message.message_id)
                {
                    if existing == message {
                        return Ok(false);
                    }
                    return Err(DomainError::ConflictingTranscriptMessage(
                        message.message_id.clone(),
                    ));
                }
                if !message.tool_calls.is_empty() {
                    if message.role != TranscriptRole::Assistant {
                        return Err(DomainError::InvalidState(
                            "only assistant messages may declare tool calls".into(),
                        ));
                    }
                    if !self.inflight_tool_call_ids.is_empty() {
                        return Err(DomainError::InvalidState(
                            "a second tool batch cannot start before the current batch finishes"
                                .into(),
                        ));
                    }
                    for call in &message.tool_calls {
                        if self.transcript.iter().any(|existing| {
                            existing.tool_calls.iter().any(|existing_call| {
                                existing_call.tool_call_id == call.tool_call_id
                            })
                        }) {
                            return Err(DomainError::DuplicateTranscriptToolCallId(
                                call.tool_call_id.clone(),
                            ));
                        }
                        self.inflight_tool_call_ids
                            .insert(call.tool_call_id.clone());
                    }
                }
                if let Some(tool_call_id) = &message.tool_call_id {
                    if message.role != TranscriptRole::Tool {
                        return Err(DomainError::InvalidState(
                            "tool_call_id may only be attached to tool messages".into(),
                        ));
                    }
                    if !self.transcript.iter().any(|existing| {
                        existing
                            .tool_calls
                            .iter()
                            .any(|call| call.tool_call_id == *tool_call_id)
                    }) {
                        return Err(DomainError::UnknownToolCall(tool_call_id.clone()));
                    }
                    if !self.inflight_tool_call_ids.remove(tool_call_id) {
                        return Err(DomainError::ToolCallAlreadyFinished(tool_call_id.clone()));
                    }
                }
                self.transcript.push(message.clone());
                if *wake_wait {
                    self.active_wait = None;
                    self.active_timer = None;
                    self.wake_pending_wait_id = None;
                }
            }
            SessionEvent::ActivationStarted {
                activation_id,
                selection,
                started_at_ms,
            } => {
                if self.active_activation.is_some() {
                    return Err(DomainError::InvalidState(
                        "session already has an active activation".into(),
                    ));
                }
                if selection != &self.selection {
                    return Err(DomainError::InvalidState(
                        "activation selection does not match session selection".into(),
                    ));
                }
                self.active_activation = Some(ActiveActivation {
                    activation_id: activation_id.clone(),
                    selection: selection.clone(),
                    started_at_ms: *started_at_ms,
                });
                self.last_activation_outcome = None;
                self.active_model_round = None;
            }
            SessionEvent::ModelRoundStarted {
                activation_id,
                round_id,
                purpose,
                mailbox_through_seq,
                started_at_ms,
            } => {
                let activation = self.active_activation.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("model round has no activation".into())
                })?;
                if activation.activation_id != *activation_id {
                    return Err(DomainError::InvalidState(
                        "model round belongs to another activation".into(),
                    ));
                }
                if let Some(existing) = &self.active_model_round {
                    let completed = existing
                        .attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.outcome == ModelAttemptOutcome::Completed);
                    if !completed {
                        return Err(DomainError::InvalidState(
                            "session already has an active model round".into(),
                        ));
                    }
                    // A completed request is a round boundary. The next
                    // round replaces that completed projection while keeping
                    // its immutable facts in the event stream.
                    self.active_model_round = None;
                }
                self.active_model_round = Some(ActiveModelRound {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    purpose: purpose.clone(),
                    mailbox_through_seq: *mailbox_through_seq,
                    started_at_ms: *started_at_ms,
                    request: None,
                    attempt: None,
                    retry: None,
                });
            }
            SessionEvent::ContextHandoffPlanned { plan } => {
                let activation = self.active_activation.as_ref().ok_or_else(|| {
                    DomainError::InvalidState(
                        "context handoff plan has no active activation".into(),
                    )
                })?;
                if plan.activation_id != activation.activation_id
                    || activation.selection != plan.selection
                {
                    return Err(DomainError::InvalidState(
                        "context handoff plan belongs to another activation".into(),
                    ));
                }
                if plan.previous_handoff_id.as_deref()
                    != self
                        .latest_context_handoff
                        .as_ref()
                        .map(|handoff| handoff.handoff_id.as_str())
                {
                    return Err(DomainError::InvalidState(
                        "context handoff plan has a stale parent".into(),
                    ));
                }
                let expected_generation = self
                    .latest_context_handoff
                    .as_ref()
                    .map_or(2, |handoff| handoff.next_generation.saturating_add(1));
                if plan.next_generation != expected_generation {
                    return Err(DomainError::InvalidState(
                        "context handoff plan has an invalid generation".into(),
                    ));
                }
                if self
                    .transcript
                    .last()
                    .map(|message| message.message_id.as_str())
                    != Some(plan.covered_through_message_id.as_str())
                {
                    return Err(DomainError::InvalidState(
                        "context handoff plan boundary is not the latest live message".into(),
                    ));
                }
                if let Some(existing) = &self.pending_context_handoff {
                    if existing == plan {
                        return Ok(false);
                    }
                    return Err(DomainError::InvalidState(
                        "context handoff already has a pending plan".into(),
                    ));
                }
                self.pending_context_handoff = Some(plan.clone());
                self.last_context_handoff_failure = None;
            }
            SessionEvent::ContextHandoffCreated { handoff } => {
                let plan = self.pending_context_handoff.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("context handoff document has no pending plan".into())
                })?;
                if handoff.plan_id != plan.plan_id
                    || handoff.previous_handoff_id != plan.previous_handoff_id
                    || handoff.next_generation != plan.next_generation
                    || handoff.covered_through_message_id != plan.covered_through_message_id
                    || handoff.selection != plan.selection
                {
                    return Err(DomainError::InvalidState(
                        "context handoff document conflicts with its durable plan".into(),
                    ));
                }
                let projected = ContextHandoffState::from(handoff);
                if let Some(existing) = &self.latest_context_handoff {
                    if existing.matches_document(handoff) {
                        self.pending_context_handoff = None;
                        return Ok(true);
                    }
                    if Some(existing.handoff_id.as_str()) != handoff.previous_handoff_id.as_deref()
                    {
                        return Err(DomainError::InvalidState(
                            "context handoff document has a stale parent".into(),
                        ));
                    }
                }
                let boundary = self
                    .transcript
                    .iter()
                    .position(|message| message.message_id == handoff.covered_through_message_id)
                    .ok_or_else(|| {
                        DomainError::InvalidState(
                            "context handoff boundary is absent from live history".into(),
                        )
                    })?;
                self.transcript.drain(..=boundary);
                self.latest_context_handoff = Some(projected);
                self.latest_model_usage = None;
                self.last_model_attempt_failure = None;
                self.last_model_attempts_exhausted = None;
                self.pending_context_handoff = None;
                self.last_context_handoff_failure = None;
            }
            SessionEvent::ContextHandoffRejected { plan_id, .. } => {
                let plan = self.pending_context_handoff.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("rejected context handoff has no pending plan".into())
                })?;
                if plan.plan_id != *plan_id {
                    return Err(DomainError::InvalidState(
                        "rejected context handoff belongs to another plan".into(),
                    ));
                }
                let round = self.active_model_round.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("rejected context handoff has no model round".into())
                })?;
                if round.purpose != ModelRequestPurpose::ContextHandoff
                    || !round
                        .attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.outcome == ModelAttemptOutcome::Completed)
                {
                    return Err(DomainError::InvalidState(
                        "context handoff may only reject after its model request completes".into(),
                    ));
                }
            }
            SessionEvent::ContextHandoffFailed { plan_id, error, .. } => {
                let plan = self.pending_context_handoff.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("context handoff failure has no pending plan".into())
                })?;
                if plan.plan_id != *plan_id {
                    return Err(DomainError::InvalidState(
                        "context handoff failure belongs to another plan".into(),
                    ));
                }
                self.pending_context_handoff = None;
                self.last_context_handoff_failure = Some(error.clone());
            }
            SessionEvent::ModelRequestDeclared {
                activation_id,
                round_id,
                request_id,
                request_fingerprint,
                prompt_fingerprint,
                tool_schema_fingerprint,
                maximum_attempts,
            } => {
                let round = self.active_model_round.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("model request has no active round".into())
                })?;
                if round.activation_id != *activation_id || round.round_id != *round_id {
                    return Err(DomainError::InvalidState(
                        "model request belongs to another round".into(),
                    ));
                }
                if round.request.is_some() {
                    return Err(DomainError::InvalidState(
                        "model request was prepared more than once".into(),
                    ));
                }
                round.request = Some(ModelRequestFact {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    request_fingerprint: request_fingerprint.clone(),
                    prompt_fingerprint: prompt_fingerprint.clone(),
                    tool_schema_fingerprint: tool_schema_fingerprint.clone(),
                    maximum_attempts: *maximum_attempts,
                });
            }
            SessionEvent::ModelAttemptStarted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                started_at_ms,
            } => {
                let round = self.active_model_round.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("model attempt has no active round".into())
                })?;
                let request = round.request.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("model attempt has no declared request".into())
                })?;
                if request.activation_id != *activation_id
                    || request.round_id != *round_id
                    || request.request_id != *request_id
                {
                    return Err(DomainError::InvalidState(
                        "model attempt belongs to another request".into(),
                    ));
                }
                if *attempt_number > request.maximum_attempts {
                    return Err(DomainError::InvalidState(
                        "model attempt exceeds declared request budget".into(),
                    ));
                }
                if let Some(existing) = &round.attempt {
                    if existing.attempt_id == *attempt_id
                        && existing.attempt_number == *attempt_number
                    {
                        return Ok(false);
                    }
                    return Err(DomainError::InvalidState(
                        "model request already has an attempt".into(),
                    ));
                }
                if let Some(schedule) = &round.retry {
                    if schedule.next_attempt_number != *attempt_number {
                        return Err(DomainError::InvalidState(
                            "model attempt does not claim the scheduled retry".into(),
                        ));
                    }
                } else if *attempt_number != 1 {
                    return Err(DomainError::InvalidState(
                        "first model attempt must have number one".into(),
                    ));
                }
                round.attempt = Some(ModelAttemptRecord {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    attempt_id: attempt_id.clone(),
                    attempt_number: *attempt_number,
                    started_at_ms: *started_at_ms,
                    outcome: ModelAttemptOutcome::Running,
                    failure: None,
                });
                round.retry = None;
            }
            SessionEvent::ModelAttemptFailedFact {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                error_class,
                retryable,
                provider_input: _,
            } => {
                let attempt = current_model_attempt_mut(
                    self,
                    activation_id,
                    round_id,
                    request_id,
                    attempt_id,
                    *attempt_number,
                )?;
                let failure = ModelAttemptFailureCause {
                    error_class: error_class.clone(),
                    retryable: *retryable,
                };
                match attempt.outcome {
                    ModelAttemptOutcome::Running => {
                        attempt.outcome = ModelAttemptOutcome::Failed;
                        attempt.failure = Some(failure);
                    }
                    ModelAttemptOutcome::Failed if attempt.failure.as_ref() == Some(&failure) => {
                        return Ok(false)
                    }
                    ModelAttemptOutcome::Failed => {
                        return Err(DomainError::InvalidState(
                            "model attempt has conflicting failure facts".into(),
                        ))
                    }
                    _ => {
                        return Err(DomainError::InvalidState(
                            "model attempt failure is not first-wins".into(),
                        ))
                    }
                }
            }
            SessionEvent::ModelAttemptInterrupted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                ..
            } => {
                let attempt = current_model_attempt_mut(
                    self,
                    activation_id,
                    round_id,
                    request_id,
                    attempt_id,
                    *attempt_number,
                )?;
                match attempt.outcome {
                    ModelAttemptOutcome::Running => {
                        attempt.outcome = ModelAttemptOutcome::Interrupted
                    }
                    ModelAttemptOutcome::Interrupted => return Ok(false),
                    _ => {
                        return Err(DomainError::InvalidState(
                            "model attempt interruption is not first-wins".into(),
                        ))
                    }
                }
            }
            SessionEvent::ModelRequestAbandoned {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                ..
            } => {
                let round = self.active_model_round.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("abandonment has no active model round".into())
                })?;
                let request = round.request.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("abandonment has no declared request".into())
                })?;
                let attempt = round.attempt.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("abandonment has no model attempt".into())
                })?;
                if round.activation_id != *activation_id
                    || round.round_id != *round_id
                    || request.request_id != *request_id
                    || attempt.attempt_id != *attempt_id
                {
                    return Err(DomainError::InvalidState(
                        "abandonment belongs to another model request".into(),
                    ));
                }
                if !matches!(
                    attempt.outcome,
                    ModelAttemptOutcome::Failed | ModelAttemptOutcome::Interrupted
                ) {
                    return Err(DomainError::InvalidState(
                        "only a failed or interrupted model request can be abandoned".into(),
                    ));
                }
                let invalidates_context_handoff = round.purpose
                    == ModelRequestPurpose::ContextHandoff
                    && !self.mailbox.is_empty();
                self.active_model_round = None;
                if invalidates_context_handoff {
                    self.pending_context_handoff = None;
                }
            }
            SessionEvent::ModelAttemptsExhausted { fact } => {
                if let Some(existing) = &self.last_model_attempts_exhausted {
                    if existing == fact {
                        return Ok(false);
                    }
                    if existing.activation_id == fact.activation_id {
                        return Err(DomainError::InvalidState(
                            "model exhaustion has conflicting semantics".into(),
                        ));
                    }
                }
                let round = self.active_model_round.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("model exhaustion has no active round".into())
                })?;
                let request = round.request.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("model exhaustion has no declared request".into())
                })?;
                let attempt = round.attempt.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("model exhaustion has no active attempt".into())
                })?;
                if round.activation_id != fact.activation_id
                    || round.round_id != fact.round_id
                    || request.request_id != fact.request_id
                    || attempt.attempt_id != fact.attempt_id
                    || attempt.attempt_number != fact.attempt_number
                    || request.maximum_attempts != fact.maximum_attempts
                {
                    return Err(DomainError::InvalidState(
                        "model exhaustion belongs to another attempt".into(),
                    ));
                }
                if !matches!(
                    attempt.outcome,
                    ModelAttemptOutcome::Failed | ModelAttemptOutcome::Interrupted
                ) {
                    return Err(DomainError::InvalidState(
                        "model exhaustion requires a failed or interrupted attempt".into(),
                    ));
                }
                self.last_model_attempts_exhausted = Some(fact.clone());
            }
            SessionEvent::ModelStepRetryScheduled { schedule } => {
                let round = self.active_model_round.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("retry has no active model round".into())
                })?;
                let attempt = round.attempt.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("retry has no model attempt".into())
                })?;
                if attempt.activation_id != schedule.activation_id
                    || attempt.round_id != schedule.round_id
                    || attempt.request_id != schedule.request_id
                    || attempt.attempt_id != schedule.failed_attempt_id
                    || attempt.attempt_number != schedule.failed_attempt_number
                {
                    return Err(DomainError::InvalidState(
                        "retry schedule does not match failed attempt".into(),
                    ));
                }
                if !matches!(
                    attempt.outcome,
                    ModelAttemptOutcome::Failed | ModelAttemptOutcome::Interrupted
                ) {
                    return Err(DomainError::InvalidState(
                        "retry schedule requires a failed or interrupted attempt".into(),
                    ));
                }
                if let Some(existing) = &round.retry {
                    if existing == schedule {
                        return Ok(false);
                    }
                    return Err(DomainError::InvalidState(
                        "model retry schedule has conflicting semantics".into(),
                    ));
                }
                round.retry = Some(schedule.clone());
                round.attempt = None;
            }
            SessionEvent::ModelRequestCompleted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                usage,
                provider_input: _,
            } => {
                let round = self.active_model_round.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("model completion has no active round".into())
                })?;
                let attempt = round.attempt.as_mut().ok_or_else(|| {
                    DomainError::InvalidState("model completion has no attempt".into())
                })?;
                if attempt.activation_id != *activation_id
                    || attempt.round_id != *round_id
                    || attempt.request_id != *request_id
                    || attempt.attempt_id != *attempt_id
                {
                    return Err(DomainError::InvalidState(
                        "model completion belongs to another attempt".into(),
                    ));
                }
                match attempt.outcome {
                    ModelAttemptOutcome::Running => {
                        attempt.outcome = ModelAttemptOutcome::Completed
                    }
                    ModelAttemptOutcome::Completed => return Ok(false),
                    _ => {
                        return Err(DomainError::InvalidState(
                            "model completion requires a running attempt".into(),
                        ))
                    }
                }
                if round.purpose == ModelRequestPurpose::Conversation {
                    if let Some(usage) = usage {
                        self.latest_model_usage = Some(usage.clone());
                    }
                }
            }
            SessionEvent::ActivationFinished {
                activation_id,
                outcome,
                finished_at_ms: _,
            } => {
                let active = self.active_activation.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("activation finish has no active activation".into())
                })?;
                if active.activation_id != *activation_id {
                    return Err(DomainError::InvalidState(
                        "activation finish belongs to another activation".into(),
                    ));
                }
                self.active_activation = None;
                self.last_activation_outcome = Some(outcome.clone());
                self.active_model_round = None;
                self.pending_context_handoff = None;
            }
            SessionEvent::ModelAttemptFailed { failure } => {
                if !self.transcript.last().is_some_and(|message| {
                    message.role.is_mailbox_input()
                        && message.message_id == failure.trigger_message_id
                }) {
                    return Err(DomainError::InvalidState(
                        "model attempt failure does not match the current input message".into(),
                    ));
                }
                if let Some(existing) = &self.last_model_attempt_failure {
                    if existing.trigger_message_id == failure.trigger_message_id {
                        if existing == failure {
                            return Ok(false);
                        }
                        return Err(DomainError::InvalidState(
                            "current input message has conflicting terminal model failures".into(),
                        ));
                    }
                }
                self.last_model_attempt_failure = Some(failure.clone());
            }
            SessionEvent::WaitSet { wait } => {
                if self.active_wait.as_ref() == Some(wait)
                    && self.active_timer.is_none()
                    && self.wake_pending_wait_id.is_none()
                {
                    return Ok(false);
                }
                self.active_wait = Some(wait.clone());
                self.active_timer = None;
                self.wake_pending_wait_id = None;
            }
            SessionEvent::WaitTimerScheduled { timer } => {
                let wait = self.active_wait.as_ref().ok_or_else(|| {
                    DomainError::InvalidState("wait timer has no active wait".into())
                })?;
                if wait.wait_id != timer.wait_id || wait.deadline_ms != timer.deadline_ms {
                    return Err(DomainError::InvalidState(
                        "wait timer does not match active wait".into(),
                    ));
                }
                if self.active_timer.as_ref() == Some(timer) {
                    return Ok(false);
                }
                self.active_timer = Some(timer.clone());
            }
            SessionEvent::WaitCleared { wait_id } | SessionEvent::WaitExpired { wait_id } => {
                if self.wake_pending_wait_id.as_deref() == Some(wait_id.as_str()) {
                    return Ok(false);
                }
                if self
                    .active_wait
                    .as_ref()
                    .is_some_and(|wait| wait.wait_id == *wait_id)
                {
                    self.active_wait = None;
                    self.active_timer = None;
                    self.wake_pending_wait_id = None;
                } else {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn validate_event_position(&self, event: &SessionEvent) -> Result<(), DomainError> {
        match (self.stream_version, event) {
            (0, SessionEvent::SessionCreated { .. }) => Ok(()),
            (0, _) => Err(DomainError::SessionNotCreated),
            (_, SessionEvent::SessionCreated { .. }) => Err(DomainError::SessionAlreadyCreated),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("wait timeout must be between {WAIT_MIN_SECONDS} and {WAIT_MAX_SECONDS} seconds")]
    InvalidWaitTimeout,
    #[error("event batch must not be empty")]
    EmptyEventBatch,
    #[error("session mismatch: expected {expected}, got {actual}")]
    SessionMismatch { expected: String, actual: String },
    #[error("stream version gap: expected {expected}, got {actual}")]
    StreamVersionGap {
        expected: StreamVersion,
        actual: StreamVersion,
    },
    #[error("unsupported event schema version: {0}")]
    UnsupportedEventSchema(u32),
    #[error("unsupported SessionCreated schema version: {0}")]
    UnsupportedSessionCreatedSchema(u32),
    #[error("session stream does not begin with SessionCreated")]
    SessionNotCreated,
    #[error("SessionCreated can only be the first stream event")]
    SessionAlreadyCreated,
    #[error("session creation time must not be negative")]
    InvalidCreatedAt,
    #[error("{field} must not be negative")]
    InvalidTimestamp { field: &'static str },
    #[error("timestamp order is invalid: {start} is after {end}")]
    InvalidTimestampOrder { start: i64, end: i64 },
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("mailbox message {0} has conflicting semantics")]
    ConflictingMailboxMessage(String),
    #[error("mailbox sequence expected {expected}, got {actual}")]
    MailboxOrder { expected: u64, actual: u64 },
    #[error("mailbox drain through {requested} exceeds available sequence {available}")]
    MailboxDrainBeyondAvailable { requested: u64, available: u64 },
    #[error("transcript message {0} has conflicting semantics")]
    ConflictingTranscriptMessage(String),
    #[error("transcript tool_call_id appears more than once: {0}")]
    DuplicateTranscriptToolCallId(String),
    #[error("tool call {0} is not declared by an assistant message")]
    UnknownToolCall(String),
    #[error("tool call {0} already has a terminal Tool message")]
    ToolCallAlreadyFinished(String),
    #[error("stream version overflow")]
    VersionOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created_active_state() -> SessionState {
        let selection = SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "off".to_owned(),
        };
        SessionState::new("session")
            .apply_events([
                SessionEvent::SessionCreated {
                    schema_version: SESSION_CREATED_SCHEMA_VERSION,
                    session_id: "session".to_owned(),
                    created_at_ms: 1,
                    selection: selection.clone(),
                    system_prompt: None,
                    workspace: "/workspace".to_owned(),
                },
                SessionEvent::ActivationStarted {
                    activation_id: "activation".to_owned(),
                    selection,
                    started_at_ms: 2,
                },
            ])
            .unwrap()
    }

    fn assistant_message(message_id: &str, tool_calls: Vec<ToolCall>) -> TranscriptMessage {
        TranscriptMessage {
            message_id: message_id.to_owned(),
            role: TranscriptRole::Assistant,
            content: Arc::from(""),
            is_error: false,
            tool_call_id: None,
            tool_calls,
            provider_context: None,
            source_mailbox_seq: None,
        }
    }

    #[test]
    fn assistant_only_round_is_followup_work_until_activation_finishes() {
        let state = created_active_state()
            .apply_event(&SessionEvent::MessageAppended {
                message: assistant_message("assistant", Vec::new()),
                wake_wait: false,
            })
            .unwrap();
        assert_eq!(
            state.model_followup_identity().as_deref(),
            Some("assistant")
        );
        assert!(state.is_startup_runnable());

        let finished = state
            .apply_event(&SessionEvent::ActivationFinished {
                activation_id: "activation".to_owned(),
                outcome: ActivationOutcome::Finished,
                finished_at_ms: 3,
            })
            .unwrap();
        assert_eq!(finished.model_followup_identity(), None);
        assert!(!finished.is_startup_runnable());
    }

    #[test]
    fn rejected_handoff_preserves_the_plan_and_does_not_advance_the_usage_anchor() {
        let mut state = created_active_state()
            .apply_event(&SessionEvent::MessageAppended {
                message: TranscriptMessage {
                    message_id: "user".to_owned(),
                    role: TranscriptRole::User,
                    content: Arc::from("continue"),
                    is_error: false,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    provider_context: None,
                    source_mailbox_seq: None,
                },
                wake_wait: false,
            })
            .unwrap();
        let previous_usage = ModelUsageAnchor {
            context_generation: 1,
            selection_fingerprint: "selection-fingerprint".to_owned(),
            tool_schema_fingerprint: "tool-fingerprint".to_owned(),
            result_event_id: Some("user".to_owned()),
            input_tokens: 1_000,
            cached_input_tokens: Some(900),
            output_tokens: 100,
            output_reasoning_tokens: Some(80),
            output_text_tokens: Some(20),
        };
        state.latest_model_usage = Some(previous_usage.clone());
        let plan = ContextHandoffPlan {
            plan_id: "plan".to_owned(),
            activation_id: "activation".to_owned(),
            previous_handoff_id: None,
            next_generation: 2,
            covered_through_message_id: "user".to_owned(),
            max_output_tokens: 5_000,
            selection: state.selection.clone(),
        };
        state = state
            .apply_events([
                SessionEvent::ContextHandoffPlanned { plan: plan.clone() },
                SessionEvent::ModelRoundStarted {
                    activation_id: "activation".to_owned(),
                    round_id: "round".to_owned(),
                    purpose: ModelRequestPurpose::ContextHandoff,
                    mailbox_through_seq: 0,
                    started_at_ms: 3,
                },
                SessionEvent::ModelRequestDeclared {
                    activation_id: "activation".to_owned(),
                    round_id: "round".to_owned(),
                    request_id: "request".to_owned(),
                    request_fingerprint: "request-fingerprint".to_owned(),
                    prompt_fingerprint: "prompt-fingerprint".to_owned(),
                    tool_schema_fingerprint: "tool-fingerprint".to_owned(),
                    maximum_attempts: 1,
                },
                SessionEvent::ModelAttemptStarted {
                    activation_id: "activation".to_owned(),
                    round_id: "round".to_owned(),
                    request_id: "request".to_owned(),
                    attempt_id: "attempt".to_owned(),
                    attempt_number: 1,
                    started_at_ms: 4,
                },
                SessionEvent::ModelRequestCompleted {
                    activation_id: "activation".to_owned(),
                    round_id: "round".to_owned(),
                    request_id: "request".to_owned(),
                    attempt_id: "attempt".to_owned(),
                    usage: Some(ModelUsageAnchor {
                        context_generation: 1,
                        selection_fingerprint: "selection-fingerprint".to_owned(),
                        tool_schema_fingerprint: "tool-fingerprint".to_owned(),
                        result_event_id: None,
                        input_tokens: 1_200,
                        cached_input_tokens: Some(1_100),
                        output_tokens: 50,
                        output_reasoning_tokens: Some(40),
                        output_text_tokens: Some(10),
                    }),
                    provider_input: None,
                },
                SessionEvent::ContextHandoffRejected {
                    plan_id: "plan".to_owned(),
                    assistant_content: "I will inspect another file.".to_owned(),
                    tool_calls: vec![ToolCall {
                        tool_call_id: "call".to_owned(),
                        tool_name: "read".to_owned(),
                        arguments: serde_json::json!({"path": "README.md"}),
                    }],
                    provider_context: None,
                },
            ])
            .unwrap();

        assert_eq!(state.pending_context_handoff.as_ref(), Some(&plan));
        assert_eq!(state.latest_model_usage.as_ref(), Some(&previous_usage));
        assert_eq!(state.transcript.len(), 1);

        state = state
            .apply_events([
                SessionEvent::MailboxMessageAppended {
                    message: MailboxMessage {
                        message_id: "steer".to_owned(),
                        mailbox_seq: 1,
                        content: Arc::from("new instruction"),
                        received_at_ms: 5,
                    },
                },
                SessionEvent::MailboxDrained {
                    through_mailbox_seq: 1,
                },
            ])
            .unwrap();
        assert!(state.pending_context_handoff.is_none());
        assert_eq!(state.transcript.last().unwrap().message_id, "steer");
    }

    #[test]
    fn expired_wait_resumes_its_tool_round_but_finished_activation_does_not() {
        let wait = ActiveWait {
            wait_id: "wait".to_owned(),
            reason: "await input".to_owned(),
            timeout_seconds: WAIT_MIN_SECONDS,
            deadline_ms: 1_000,
            source: WaitSource::WaitFor,
            tool_call_ids: vec!["wait-call".to_owned()],
        };
        let state = created_active_state()
            .apply_events([
                SessionEvent::MessageAppended {
                    message: assistant_message(
                        "assistant-wait",
                        vec![ToolCall {
                            tool_call_id: "wait-call".to_owned(),
                            tool_name: "wait_for".to_owned(),
                            arguments: serde_json::json!({
                                "reason": "await input",
                                "timeout_seconds": WAIT_MIN_SECONDS,
                            }),
                        }],
                    ),
                    wake_wait: false,
                },
                SessionEvent::MessageAppended {
                    message: TranscriptMessage {
                        message_id: "wait-result".to_owned(),
                        role: TranscriptRole::Tool,
                        content: Arc::from("wait_for accepted"),
                        is_error: false,
                        tool_call_id: Some("wait-call".to_owned()),
                        tool_calls: Vec::new(),
                        provider_context: None,
                        source_mailbox_seq: None,
                    },
                    wake_wait: false,
                },
                SessionEvent::WaitSet { wait: wait.clone() },
                SessionEvent::WaitTimerScheduled {
                    timer: WaitTimerIntent {
                        wait_id: wait.wait_id.clone(),
                        deadline_ms: wait.deadline_ms,
                    },
                },
                SessionEvent::ActivationFinished {
                    activation_id: "activation".to_owned(),
                    outcome: ActivationOutcome::Wait,
                    finished_at_ms: 3,
                },
            ])
            .unwrap();
        assert_eq!(state.model_followup_identity(), None);

        let expired = state
            .apply_event(&SessionEvent::WaitExpired {
                wait_id: wait.wait_id,
            })
            .unwrap();
        assert_eq!(
            expired.model_followup_identity().as_deref(),
            Some("assistant-wait")
        );
        assert!(expired.is_startup_runnable());
    }

    #[test]
    fn mailbox_does_not_make_a_session_runnable_while_a_tool_effect_is_running() {
        let mut state = SessionState::new("session");
        state.mailbox.push(MailboxMessage {
            message_id: "message".to_owned(),
            mailbox_seq: 1,
            content: Arc::from("during tool"),
            received_at_ms: 1,
        });
        state.inflight_tool_call_ids.insert("tool-call".to_owned());

        assert!(!state.is_startup_runnable());
    }

    #[test]
    fn failed_model_attempt_keeps_the_recovery_cause_in_state() {
        let selection = SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "off".to_owned(),
        };
        let mut state = SessionState::new("session");
        for event in [
            SessionEvent::SessionCreated {
                schema_version: SESSION_CREATED_SCHEMA_VERSION,
                session_id: "session".to_owned(),
                created_at_ms: 1,
                selection: selection.clone(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            },
            SessionEvent::ActivationStarted {
                activation_id: "activation".to_owned(),
                selection,
                started_at_ms: 2,
            },
            SessionEvent::ModelRoundStarted {
                activation_id: "activation".to_owned(),
                round_id: "round".to_owned(),
                purpose: ModelRequestPurpose::Conversation,
                mailbox_through_seq: 0,
                started_at_ms: 3,
            },
            SessionEvent::ModelRequestDeclared {
                activation_id: "activation".to_owned(),
                round_id: "round".to_owned(),
                request_id: "request".to_owned(),
                request_fingerprint: "request-fingerprint".to_owned(),
                prompt_fingerprint: "prompt-fingerprint".to_owned(),
                tool_schema_fingerprint: "tool-fingerprint".to_owned(),
                maximum_attempts: 3,
            },
            SessionEvent::ModelAttemptStarted {
                activation_id: "activation".to_owned(),
                round_id: "round".to_owned(),
                request_id: "request".to_owned(),
                attempt_id: "attempt".to_owned(),
                attempt_number: 1,
                started_at_ms: 4,
            },
            SessionEvent::ModelAttemptFailedFact {
                activation_id: "activation".to_owned(),
                round_id: "round".to_owned(),
                request_id: "request".to_owned(),
                attempt_id: "attempt".to_owned(),
                attempt_number: 1,
                error_class: "provider_unavailable".to_owned(),
                retryable: true,
                provider_input: None,
            },
        ] {
            state = state.apply_event(&event).unwrap();
        }

        let attempt = state
            .active_model_round
            .as_ref()
            .and_then(|round| round.attempt.as_ref())
            .unwrap();
        assert_eq!(attempt.outcome, ModelAttemptOutcome::Failed);
        assert_eq!(
            attempt.failure,
            Some(ModelAttemptFailureCause {
                error_class: "provider_unavailable".to_owned(),
                retryable: true,
            })
        );
        let restored: SessionState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, state);
    }
}
