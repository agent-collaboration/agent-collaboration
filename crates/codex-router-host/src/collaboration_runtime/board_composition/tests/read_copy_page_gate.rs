//! A test-only ordering signal after a real page receipt; it supplies no data or result.
use durable_steps::CallFailure;
use fleet_sync_protocol::ReplicationPosition;
use fleet_sync_receiver::InstallPageReceipt;
use std::{path::PathBuf, time::Duration};
use tokio::sync::{Mutex, oneshot, watch};

pub(in crate::collaboration_runtime::board_composition) struct StagedPageObservation {
    pub receipt: InstallPageReceipt,
    pub target: PathBuf,
}
pub(in crate::collaboration_runtime::board_composition) struct ReadCopyPause {
    pub staged: oneshot::Receiver<StagedPageObservation>,
    pub proceed: oneshot::Sender<()>,
    pub moved: watch::Receiver<Option<ReplicationPosition>>,
}
struct PendingPause {
    staged: oneshot::Sender<StagedPageObservation>,
    proceed: oneshot::Receiver<()>,
}
pub(super) struct StagedPageGate {
    pending: Mutex<Option<PendingPause>>,
    moved: watch::Sender<Option<ReplicationPosition>>,
}
impl StagedPageGate {
    pub fn new() -> (Self, ReadCopyPause) {
        let (staged, observed) = oneshot::channel();
        let (proceed, proceeding) = oneshot::channel();
        let (moved, movement) = watch::channel(None);
        (
            Self {
                pending: Mutex::new(Some(PendingPause {
                    staged,
                    proceed: proceeding,
                })),
                moved,
            },
            ReadCopyPause {
                staged: observed,
                proceed,
                moved: movement,
            },
        )
    }
    pub async fn after_page(
        &self,
        receipt: InstallPageReceipt,
        target: PathBuf,
    ) -> Result<(), CallFailure> {
        let pending = self.pending.lock().await.take();
        if let Some(pending) = pending {
            pending
                .staged
                .send(StagedPageObservation { receipt, target })
                .map_err(|_| CallFailure::AnswerLost)?;
            tokio::time::timeout(Duration::from_secs(30), pending.proceed)
                .await
                .map_err(|_| CallFailure::AnswerLost)?
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        Ok(())
    }
    pub fn moved(&self, position: ReplicationPosition) {
        self.moved.send_replace(Some(position));
    }
}
