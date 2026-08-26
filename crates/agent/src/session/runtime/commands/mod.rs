use std::{fs, sync::Arc};

use crate::session::state::{
    EventDraft, EventRecord, MailboxMessage, SessionEvent, SessionSelection, SessionState,
};

use super::{
    AppendResult, ProfileResolveError, RehydrateError, Runtime, RuntimeCommandError,
    RuntimeStreamSubscription, SessionCreate, SessionCreateResult, SessionListCursor,
    SessionListPage, StoreError,
};

fn store_error(error: StoreError) -> RuntimeCommandError {
    match error {
        StoreError::OptimisticConcurrency { .. } | StoreError::EventBatchConflict { .. } => {
            RuntimeCommandError::Conflict
        }
        StoreError::SessionNotFound => RuntimeCommandError::NotFound,
        StoreError::EmptyField { .. } | StoreError::Domain(_) => {
            RuntimeCommandError::Invalid("invalid request")
        }
        _ => RuntimeCommandError::Backend,
    }
}

fn rehydrate_error(error: RehydrateError) -> RuntimeCommandError {
    match error {
        RehydrateError::Store(StoreError::SessionNotFound) => RuntimeCommandError::NotFound,
        _ => RuntimeCommandError::Backend,
    }
}

fn read_store_error(error: StoreError) -> RuntimeCommandError {
    match error {
        StoreError::SessionNotFound => RuntimeCommandError::NotFound,
        _ => RuntimeCommandError::Backend,
    }
}

fn profile_selection_error(error: ProfileResolveError) -> RuntimeCommandError {
    match error {
        ProfileResolveError::InvalidSelection | ProfileResolveError::NotFound => {
            RuntimeCommandError::Invalid("invalid profile selection")
        }
        ProfileResolveError::AuthUnavailable => RuntimeCommandError::ProfileUnavailable,
        ProfileResolveError::Backend => RuntimeCommandError::Backend,
    }
}

fn append_mailbox_blocking(
    runtime: &Runtime,
    session_id: &str,
    content: String,
) -> Result<(AppendResult, SessionState, MailboxMessage), RuntimeCommandError> {
    let received_at_ms = runtime.clock.now_ms();
    let mut current = runtime
        .store
        .rehydrate_verified(session_id)
        .map_err(rehydrate_error)?;
    loop {
        let mailbox_seq = current
            .consumed_through_mailbox_seq
            .checked_add(current.mailbox.len() as u64 + 1)
            .ok_or(RuntimeCommandError::Invalid("mailbox is full"))?;
        let events = vec![EventDraft::identified(|message_id| {
            SessionEvent::MailboxMessageAppended {
                message: MailboxMessage {
                    message_id: message_id.to_owned(),
                    mailbox_seq,
                    content: content.clone().into(),
                    received_at_ms,
                },
            }
        })];
        match runtime.store.append_verified(session_id, current, &events) {
            Ok(appended) => {
                let message = appended
                    .state
                    .mailbox
                    .last()
                    .cloned()
                    .ok_or(RuntimeCommandError::Backend)?;
                return Ok((appended.append, appended.state.into_state(), message));
            }
            Err(StoreError::OptimisticConcurrency { .. }) => {
                current = runtime
                    .store
                    .rehydrate_verified(session_id)
                    .map_err(rehydrate_error)?;
            }
            Err(error) => return Err(store_error(error)),
        }
    }
}

fn update_selection_blocking(
    runtime: &Runtime,
    session_id: &str,
    selection: SessionSelection,
) -> Result<(Option<AppendResult>, SessionState), RuntimeCommandError> {
    runtime
        .profiles
        .model_limits(&selection)
        .map_err(profile_selection_error)?;
    let current = runtime
        .store
        .rehydrate_verified(session_id)
        .map_err(rehydrate_error)?;
    if current.selection == selection {
        return Ok((None, current.into_state()));
    }
    if current.active_activation.is_some()
        || current.active_model_round.is_some()
        || current.active_wait.is_some()
        || current.has_inflight_tool_effect()
    {
        return Err(RuntimeCommandError::Conflict);
    }
    let events = EventDraft::single(SessionEvent::SelectionChanged { selection });
    let appended = runtime
        .store
        .append_verified(session_id, current, &events)
        .map_err(store_error)?;
    Ok((Some(appended.append), appended.state.into_state()))
}

fn create_session_blocking(
    runtime: &Runtime,
    selection: SessionSelection,
    system_prompt: Option<String>,
    workspace: String,
) -> Result<SessionCreateResult, RuntimeCommandError> {
    let workspace = fs::canonicalize(workspace)
        .ok()
        .filter(|path| path.is_dir())
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or(RuntimeCommandError::Invalid(
            "workspace must be an existing directory",
        ))?;
    runtime
        .profiles
        .model_limits(&selection)
        .map_err(profile_selection_error)?;
    runtime
        .store
        .create_session(&SessionCreate {
            created_at_ms: runtime.clock.now_ms(),
            selection,
            system_prompt,
            workspace,
        })
        .map_err(store_error)
}

impl Runtime {
    pub async fn update_selection(
        self: &Arc<Self>,
        session_id: String,
        selection: SessionSelection,
    ) -> Result<SessionState, RuntimeCommandError> {
        let runtime = Arc::clone(self);
        let wake_session_id = session_id.clone();
        let (append, state) = tokio::task::spawn_blocking(move || {
            update_selection_blocking(&runtime, &session_id, selection)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)??;
        if let Some(append) = &append {
            self.observe_append(append, &state).await;
        }
        if state.is_startup_runnable() {
            self.wake(wake_session_id);
        }
        Ok(state)
    }

    pub async fn create_session(
        self: &Arc<Self>,
        selection: SessionSelection,
        system_prompt: Option<String>,
        workspace: String,
    ) -> Result<SessionCreateResult, RuntimeCommandError> {
        let runtime = Arc::clone(self);
        let operation = tokio::task::spawn_blocking(move || {
            create_session_blocking(&runtime, selection, system_prompt, workspace)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)??;
        self.observe_append(&operation.append, &operation.state)
            .await;
        Ok(operation)
    }

    pub async fn append_mailbox(
        self: &Arc<Self>,
        session_id: String,
        content: String,
    ) -> Result<(AppendResult, SessionState, MailboxMessage), RuntimeCommandError> {
        let runtime = Arc::clone(self);
        let wake_session_id = session_id.clone();
        let operation = tokio::task::spawn_blocking(move || {
            append_mailbox_blocking(&runtime, &session_id, content)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)??;
        self.observe_append(&operation.0, &operation.1).await;
        self.wake(wake_session_id);
        Ok(operation)
    }

    pub async fn list_sessions(
        &self,
        cursor: Option<SessionListCursor>,
        limit: usize,
    ) -> Result<SessionListPage, RuntimeCommandError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .list_sessions_page(cursor.as_ref(), limit)
                .map_err(list_store_error)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)?
    }

    pub async fn get_session(
        &self,
        session_id: String,
    ) -> Result<SessionState, RuntimeCommandError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .rehydrate_verified(&session_id)
                .map(|state| state.into_state())
                .map_err(rehydrate_error)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)?
    }

    /// Subscribe before reading the session suffix. A events racing the read
    /// may appear in both sources and is deduplicated by stream version.
    pub async fn subscribe_session(
        &self,
        session_id: String,
        after_version: u64,
        limit: usize,
    ) -> Result<(RuntimeStreamSubscription, Vec<EventRecord>), RuntimeCommandError> {
        let subscription = self.stream_publisher.subscribe();
        let store = self.store.clone();
        let records = tokio::task::spawn_blocking(move || {
            store
                .read_stream(&session_id, after_version, limit)
                .map_err(read_store_error)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)??;
        Ok((subscription, records))
    }

    pub async fn read_session_events(
        &self,
        session_id: String,
        after_version: u64,
        limit: usize,
    ) -> Result<Vec<EventRecord>, RuntimeCommandError> {
        let store = self.store.clone();
        let records = tokio::task::spawn_blocking(move || {
            store
                .read_stream(&session_id, after_version, limit)
                .map_err(read_store_error)
        })
        .await
        .map_err(|_| RuntimeCommandError::Backend)??;
        Ok(records)
    }
}

fn list_store_error(error: StoreError) -> RuntimeCommandError {
    match error {
        StoreError::InvalidSessionListLimit => {
            RuntimeCommandError::Invalid("invalid session list limit")
        }
        StoreError::InvalidSessionListCursor => {
            RuntimeCommandError::Invalid("invalid session list cursor")
        }
        other => store_error(other),
    }
}
