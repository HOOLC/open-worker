//! Tool-originated control requests, persisted by the session's sole writer.
use super::super::events::ToolResultData;

#[derive(Clone, Debug)]
pub struct ToolCancellation {
    pub signalled: bool,
    pub result: Option<ToolResultData>,
}

#[derive(Clone, Debug)]
pub struct ToolControl {
    sender: tokio::sync::mpsc::Sender<CancelToolRequest>,
}

impl PartialEq for ToolControl {
    fn eq(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}
impl Eq for ToolControl {}

#[derive(Debug)]
pub(crate) struct CancelToolRequest {
    pub target: String,
    pub response: tokio::sync::oneshot::Sender<Result<ToolCancellation, String>>,
}

impl ToolControl {
    pub(crate) fn channel(
        capacity: usize,
    ) -> (Self, tokio::sync::mpsc::Receiver<CancelToolRequest>) {
        let (sender, receiver) = tokio::sync::mpsc::channel(capacity.max(1));
        (Self { sender }, receiver)
    }

    pub async fn cancel(&self, target: String) -> Result<ToolCancellation, String> {
        let (response, received) = tokio::sync::oneshot::channel();
        self.sender
            .send(CancelToolRequest { target, response })
            .await
            .map_err(|_| "Session control is shutting down.".to_owned())?;
        received
            .await
            .map_err(|_| "Session control stopped before confirming cancellation.".to_owned())?
    }
}
