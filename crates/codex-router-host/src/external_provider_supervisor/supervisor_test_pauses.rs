//! Test-only pauses that hold an operation inside one of the supervisor's
//! race windows: the first receiver fires there, and sending on the returned
//! sender lets it go on.

use super::{AdmissionTestPause, ExternalProviderSupervisor, OperationId};

impl ExternalProviderSupervisor {
    /// Pauses the next queued API dispatch just after it claims its entry:
    /// the first receiver fires there, and sending on the returned sender lets
    /// it go on.
    pub(crate) fn pause_next_queued_dispatch_after_claim(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .dispatch_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    /// Pauses the next admitted dispatch with its submission spawned and its
    /// dispatch observation not yet awaited.
    pub(crate) fn pause_next_dispatch_before_observation(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .observation_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    /// Pauses the next steer with its approver records pinned, before its
    /// injection is sent.
    pub(crate) fn pause_next_steer_before_injection(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .steer_injection_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    /// Pauses the next steer after its injection reply and before it reads
    /// the joined turn's approver.
    pub(crate) fn pause_next_steer_after_reply(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .steer_reply_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }

    pub(crate) fn pause_after_result_publication(
        &self,
        operation_id: OperationId,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .result_published_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((operation_id, AdmissionTestPause { entered, release }));
        (entered_receiver, release_sender)
    }

    /// Pauses the next API send routed to the session's line, after its
    /// admission and before it enters the line.
    pub(crate) fn pause_next_api_send_before_enqueue(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .queue_enqueue_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(AdmissionTestPause { entered, release });
        (entered_receiver, release_sender)
    }
}
