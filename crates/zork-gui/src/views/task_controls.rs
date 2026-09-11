//! Task decisions operate on the persisted product revision, never a visual status badge.
use super::*;

impl RootView {
    pub(super) fn selected_task(&self) -> Option<&ProductTask> {
        let id = self.selected_session.as_deref()?;
        self.sessions
            .iter()
            .find(|s| s.session_id == id)?
            .task
            .as_ref()
    }
}
