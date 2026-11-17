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
}
