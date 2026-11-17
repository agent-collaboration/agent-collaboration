//! A committed queued message's send, owned by its dispatch (D1 state
//! machines, design note §12.2, §13.1–§13.2). The line committed and claimed
//! it; its owner writes the dispatch mark, then asks the session's cell for
//! the handover. A close, cancel or stop that won meanwhile turns the
//! handover into a withdrawal, and the owner undoes the mark and settles the
//! message not sent. Replaces the line's submission gate.

use crate::ExternalProviderRuntime;
use acp_client_runtime::QueueMarkStep;
use std::sync::Arc;

/// What happened after the dispatch mark was written.
pub(crate) enum QueuedSendStep {
    /// Send the prompt under this turn.
    Send(acp_client_runtime::QueueTurn),
    /// Nothing may be sent: undo the mark, then settle not sent.
    Withdraw,
}

#[derive(Clone)]
pub(crate) struct QueuedSend {
    runtime: Arc<ExternalProviderRuntime>,
    provider_session_id: String,
    sequence: u64,
}

impl QueuedSend {
    pub(crate) fn new(
        runtime: Arc<ExternalProviderRuntime>,
        provider_session_id: String,
        sequence: u64,
    ) -> Self {
        Self {
            runtime,
            provider_session_id,
            sequence,
        }
    }

    /// The dispatch mark is written: the handover or a withdrawal, decided
    /// under the session's lock with the transport's state.
    pub(crate) fn mark_written(&self) -> QueuedSendStep {
        match self
            .runtime
            .queue_mark_written(&self.provider_session_id, self.sequence)
        {
            QueueMarkStep::Send { turn } => QueuedSendStep::Send(turn),
            QueueMarkStep::Withdraw | QueueMarkStep::EntryEnded => QueuedSendStep::Withdraw,
        }
    }

    /// The dispatch mark couldn't be written (`uncertain`: its outcome is
    /// unknown). Returns the not-sent reason.
    pub(crate) fn mark_not_written(&self, uncertain: bool) -> &'static str {
        self.runtime
            .queue_mark_not_written(&self.provider_session_id, self.sequence, uncertain)
            .unwrap_or("the dispatch couldn't be recorded")
    }

    /// A withdrawn send's mark was undone (`unmarked`) or couldn't be.
    /// Returns the reason the message settles not sent with.
    pub(crate) fn unmark_done(&self, unmarked: bool) -> &'static str {
        self.runtime
            .queue_unmark_done(&self.provider_session_id, self.sequence, unmarked)
            .unwrap_or("session closed")
    }
}
