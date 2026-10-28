//! One exact typed grant can be lost at the fixture's existing Access bridge.
use crate::FixtureResult;
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{
    ClientFrame, DatabaseId, Epoch, FrameId, HolderChangeAnswer, LeaseAnswer, LeaseRequest,
    OperationAnswer, OperationRequest, ReplicationPosition, RequestId, ServerFrame, SyncTime,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DroppedClaimGrant {
    pub frame: FrameId,
    pub attempt: RequestId,
    pub epoch: Epoch,
    pub expires_at: SyncTime,
    pub start: ReplicationPosition,
}

struct Selection {
    attempt: RequestId,
    database: Option<DatabaseId>,
    frames: BTreeSet<FrameId>,
    attempts: BTreeSet<RequestId>,
    dropped: watch::Sender<Option<DroppedClaimGrant>>,
}
#[derive(Clone, Default)]
pub(crate) struct ClaimGrantDrop(Arc<Mutex<Option<Selection>>>);

pub struct ClaimGrantDropObservation {
    probe: ClaimGrantDrop,
    dropped: watch::Receiver<Option<DroppedClaimGrant>>,
}

impl ClaimGrantDrop {
    pub(crate) fn begin(&self, attempt: RequestId) -> FixtureResult<ClaimGrantDropObservation> {
        let mut state = self.0.lock().map_err(|_| "claim selector poisoned")?;
        if state.is_some() {
            return Err("claim grant drops must not overlap".into());
        }
        let (dropped, waiting) = watch::channel(None);
        *state = Some(Selection {
            attempt,
            database: None,
            frames: BTreeSet::new(),
            attempts: BTreeSet::new(),
            dropped,
        });
        Ok(ClaimGrantDropObservation {
            probe: self.clone(),
            dropped: waiting,
        })
    }

    pub(crate) fn observe_request(&self, bytes: &[u8]) -> FixtureResult {
        let mut state = self.0.lock().map_err(|_| "claim selector poisoned")?;
        let Some(selected) = state.as_mut() else {
            return Ok(());
        };
        if let ClientFrame::Request {
            frame,
            body: OperationRequest::Lease(LeaseRequest::Claim { attempt, database }),
        } = serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(bytes)?
            && (attempt == selected.attempt || selected.database == Some(database))
        {
            selected.database = Some(database);
            selected.attempts.insert(attempt);
            if attempt == selected.attempt {
                selected.frames.insert(frame);
            }
            if selected.frames.len() > 64 || selected.attempts.len() > 64 {
                return Err("claim selector exceeded its bounded request history".into());
            }
        }
        Ok(())
    }

    pub(crate) fn drop_answer(&self, bytes: &[u8]) -> FixtureResult<bool> {
        let mut state = self.0.lock().map_err(|_| "claim selector poisoned")?;
        let Some(selected) = state.as_mut() else {
            return Ok(false);
        };
        if let ServerFrame::Answer { frame, body } =
            serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes)?
            && selected.frames.remove(&frame)
            && selected.dropped.borrow().is_none()
            && let OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                epoch,
                expires_at,
                start,
            })) = body
        {
            selected.dropped.send_replace(Some(DroppedClaimGrant {
                frame,
                attempt: selected.attempt,
                epoch,
                expires_at,
                start,
            }));
            return Ok(true);
        }
        Ok(false)
    }
}

impl ClaimGrantDropObservation {
    pub async fn wait_dropped(&mut self) -> FixtureResult<DroppedClaimGrant> {
        self.dropped
            .wait_for(Option::is_some)
            .await?
            .clone()
            .ok_or_else(|| "claim grant was not dropped".into())
    }
    pub fn observed_attempts(&self) -> FixtureResult<Vec<RequestId>> {
        let state = self.probe.0.lock().map_err(|_| "claim selector poisoned")?;
        Ok(state
            .as_ref()
            .ok_or("claim observation ended")?
            .attempts
            .iter()
            .copied()
            .collect())
    }
}
impl Drop for ClaimGrantDropObservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.probe.0.lock() {
            *state = None;
        }
    }
}
