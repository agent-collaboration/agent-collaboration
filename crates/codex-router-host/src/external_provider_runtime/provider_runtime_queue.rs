//! The Host's door to a provider session's queue line, passed through to its
//! connection's session cells (D1 state machines, design note §11.2, §12.2,
//! §13.1). The FIFO decides nothing itself: every enqueue, commit, handover,
//! cancel and stop is a transition on the session's cell.

use super::{
    ExternalProviderApprovalContext, ExternalProviderPromptOutcome, ExternalProviderRuntime,
    ExternalProviderRuntimeError, ProviderPromptDispatchObservation, host_prompt_outcome,
};
use acp_client_runtime::{
    QueueCancel, QueueEntrySink, QueueMarkStep, QueueRefusal, QueueTicket, QueueTurn,
};
use std::path::PathBuf;
use std::sync::Arc;

impl ExternalProviderRuntime {
    /// Sets the FIFO's sink for messages that moves it didn't drive end.
    pub(crate) fn set_queue_entry_sink(&self, sink: Arc<dyn QueueEntrySink>) {
        self.client.set_queue_entry_sink(sink);
    }

    pub(crate) fn queue_enqueue(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueTicket, QueueRefusal> {
        self.client.queue_enqueue(provider_session_id, operation_id)
    }

    pub(crate) fn queue_claim(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.client.queue_claim(provider_session_id, sequence)
    }

    pub(crate) fn queue_commit(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.client.queue_commit(provider_session_id, sequence)
    }

    pub(crate) fn queue_mark_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> QueueMarkStep {
        let transport_open = !self.retirement().is_cancelled();
        self.client
            .queue_mark_written(provider_session_id, sequence, transport_open)
    }

    pub(crate) fn queue_mark_not_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
        uncertain: bool,
    ) -> Option<&'static str> {
        self.client
            .queue_mark_not_written(provider_session_id, sequence, uncertain)
    }

    pub(crate) fn queue_unmark_done(
        &self,
        provider_session_id: &str,
        sequence: u64,
        unmarked: bool,
    ) -> Option<&'static str> {
        self.client
            .queue_unmark_done(provider_session_id, sequence, unmarked)
    }

    pub(crate) fn queue_cancel(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueCancel, QueueRefusal> {
        self.client.queue_cancel(provider_session_id, operation_id)
    }

    pub(crate) fn queue_stop(&self) {
        self.client.queue_stop();
    }

    /// Starts loading the session for a door and returns once its cell is
    /// Loading. The returned future finishes the load: it answers when the
    /// agent has loaded the session and its activation is published.
    pub(crate) async fn begin_load_session(
        self: &Arc<Self>,
        provider_session_id: String,
        cwd: PathBuf,
    ) -> Result<
        futures_util::future::BoxFuture<'static, Result<(), ExternalProviderRuntimeError>>,
        ExternalProviderRuntimeError,
    > {
        let started = self
            .client
            .begin_load_session(provider_session_id.clone(), cwd)
            .await?;
        let runtime = Arc::clone(self);
        Ok(Box::pin(async move {
            let generation = started.finished().await?;
            runtime
                .publish_active_session(&provider_session_id, Some(generation))
                .await
        }))
    }

    /// Sends a prompt as the session's turn (a queued one under
    /// `queue_turn`) and hands back the turn's end, which the operation's
    /// owner drops once the operation has settled (design note §13.1).
    pub(crate) async fn submit_holding_turn_end(
        &self,
        provider_session_id: String,
        input_id: session_event_model::InputId,
        contents: acp_client_runtime::ProviderTurnInput,
        context: ExternalProviderApprovalContext,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        queue_turn: Option<QueueTurn>,
    ) -> (
        Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError>,
        Option<acp_client_runtime::ProviderTurnEnd>,
    ) {
        let (result, turn_end) = self
            .client
            .submit_holding_turn_end(
                provider_session_id,
                input_id,
                contents,
                context,
                dispatch,
                queue_turn,
            )
            .await;
        (result.and_then(host_prompt_outcome), turn_end)
    }
}
