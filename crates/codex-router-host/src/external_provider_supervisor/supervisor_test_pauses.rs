//! Test-only pauses that hold an operation inside one of the supervisor's
//! race windows: the first receiver fires there, and sending on the returned
//! sender lets it go on.

use super::{AdmissionTestPause, ExternalProviderSupervisor, OperationId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReconcilePausePoint {
    BeforeStore,
    HoldingStore,
}

impl AdmissionTestPause {
    async fn wait(self) {
        let _test_ended = self.entered.send(());
        let _test_ended = self.release.await;
    }
}

impl ExternalProviderSupervisor {
    pub(super) async fn pause_before_admission_reservation_for_test(&self) {
        let pause = self
            .inner
            .admission_before_reservation_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(pause) = pause {
            pause.wait().await;
        }
    }

    pub(super) async fn pause_reconcile_for_test(&self, point: ReconcilePausePoint) {
        let pause = self
            .inner
            .reconcile_test_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_if(|(selected, _)| *selected == point);
        if let Some((_, pause)) = pause {
            pause.wait().await;
        }
    }

    pub(crate) fn tracked_operation_tasks_for_test(&self) -> tokio_util::task::TaskTracker {
        self.inner.operation_tasks.clone()
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
}
