//! The invocation-local record reload channel; no catalog or source policy.
use crate::session_picker::source::source_reload_worker::SessionRecordsReloadRequest;
use agent_session_inventory::picker_data_query::SessionsPickerDataQuery;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub(crate) struct SessionRecordsReloadPort {
    sender: tokio::sync::watch::Sender<SessionRecordsReloadRequest>,
    receiver: Arc<Mutex<Option<tokio::sync::watch::Receiver<SessionRecordsReloadRequest>>>>,
}

impl SessionRecordsReloadPort {
    pub(crate) fn new(
        initial_query: SessionsPickerDataQuery,
        sources: Vec<crate::session_picker::PickerSourceContext>,
    ) -> Self {
        let (sender, receiver) = tokio::sync::watch::channel(SessionRecordsReloadRequest {
            generation: 0,
            query: initial_query,
            sources,
        });
        Self {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }

    pub(crate) fn send(&self, request: SessionRecordsReloadRequest) {
        self.sender.send_replace(request);
    }

    pub(crate) fn take_receiver(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<SessionRecordsReloadRequest>> {
        self.receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}
