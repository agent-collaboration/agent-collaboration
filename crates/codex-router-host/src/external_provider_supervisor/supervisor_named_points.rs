//! Named points a scenario test can hold (design note §11.8), compiled only
//! with test support: the supervisor holds an operation at the point, and
//! the test releases it. The Host's approved test-support surface.

use super::{AdmissionTestPause, ExternalProviderSupervisor};

impl ExternalProviderSupervisor {
    /// Pauses `operation_id`'s settlement before its terminal write: its
    /// outcome is known, nothing is stored yet, and a prompt's turn is still
    /// held. The first receiver fires there, and sending on the returned
    /// sender lets it go on.
    pub fn pause_terminal_write_of(
        &self,
        operation_id: &collaboration_protocol::OperationId,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .terminal_write_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
            operation_id.clone(),
            AdmissionTestPause { entered, release },
        ));
        (entered_receiver, release_sender)
    }

    /// Pauses the explicit change of `target`'s mode to `value` after the
    /// agent applied it and before its record is written (Lead V6b): the
    /// session's turn for mode changes is still held. The first receiver
    /// fires there, and sending on the returned sender lets it go on.
    pub fn pause_explicit_mode_record_of(
        &self,
        target: &collaboration_protocol::SessionRef,
        value: &str,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release) = tokio::sync::oneshot::channel();
        *self
            .inner
            .explicit_mode_record_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
            target.clone(),
            value.to_owned(),
            AdmissionTestPause { entered, release },
        ));
        (entered_receiver, release_sender)
    }

    /// The next explicit mode change that has to wait for its session's turn
    /// (another change holds it, Lead V6b) fires the returned receiver as it
    /// starts waiting.
    pub fn notice_next_explicit_mode_wait(&self) -> tokio::sync::oneshot::Receiver<()> {
        self.inner.explicit_mode_order.notice_next_wait()
    }
}
