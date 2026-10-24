//! A selected Copy refusal at the existing fixture edge; no selected write is forwarded.
use crate::{BOUND, FixtureResult};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{
    ClientFrame, DatabaseId, OperationAnswer, OperationRequest, PieceRefusal, ReplicationAnswer,
    ReplicationRequest, ServerFrame, TableName,
};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

struct Selection {
    database: DatabaseId,
    table: TableName,
    references: TableName,
    requests: watch::Sender<usize>,
}
#[derive(Clone, Default)]
pub(crate) struct PieceIntegrityRefusal(Arc<Mutex<Option<Selection>>>);
pub struct PieceIntegrityObservation {
    owner: PieceIntegrityRefusal,
    requests: watch::Receiver<usize>,
}
impl PieceIntegrityRefusal {
    pub(crate) fn begin(
        &self,
        database: DatabaseId,
        table: TableName,
        references: TableName,
    ) -> FixtureResult<PieceIntegrityObservation> {
        let mut selected = self
            .0
            .lock()
            .map_err(|_| "piece refusal selector poisoned")?;
        if selected.is_some() {
            return Err("piece refusal selectors must not overlap".into());
        }
        let (requests, observed) = watch::channel(0);
        *selected = Some(Selection {
            database,
            table,
            references,
            requests,
        });
        Ok(PieceIntegrityObservation {
            owner: self.clone(),
            requests: observed,
        })
    }
    pub(crate) fn refuse_request(&self, bytes: &[u8]) -> FixtureResult<Option<Vec<u8>>> {
        let selected = self
            .0
            .lock()
            .map_err(|_| "piece refusal selector poisoned")?;
        let Some(selected) = selected.as_ref() else {
            return Ok(None);
        };
        let ClientFrame::Request {
            frame,
            body: OperationRequest::Replication(ReplicationRequest::Piece(piece)),
        } = serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(bytes)?
        else {
            return Ok(None);
        };
        if piece.database != selected.database {
            return Ok(None);
        }
        let count = selected
            .requests
            .borrow()
            .checked_add(1)
            .ok_or("piece request count overflow")?;
        selected.requests.send_replace(count);
        Ok(Some(serde_json::to_vec(&ServerFrame::<
            BoardConsumerProtocol,
        >::Answer {
            frame,
            body: OperationAnswer::Replication(ReplicationAnswer::Refused(
                PieceRefusal::IntegrityViolation {
                    table: selected.table.clone(),
                    references: selected.references.clone(),
                },
            )),
        })?))
    }
}
impl PieceIntegrityObservation {
    pub async fn wait_refused(&mut self) -> FixtureResult {
        tokio::time::timeout(BOUND, async {
            while *self.requests.borrow() == 0 {
                self.requests.changed().await?;
            }
            Ok::<(), watch::error::RecvError>(())
        })
        .await??;
        Ok(())
    }
    pub fn requests(&self) -> usize {
        *self.requests.borrow()
    }
}
impl Drop for PieceIntegrityObservation {
    fn drop(&mut self) {
        if let Ok(mut selected) = self.owner.0.lock() {
            *selected = None;
        }
    }
}
