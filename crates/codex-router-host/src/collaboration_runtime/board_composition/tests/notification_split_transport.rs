//! Delegate real replication and hold the send after an actual PartStaged was processed.
use board_fleet_sync::BoardProjectSchema;
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use fleet_sync_protocol::{
    Piece, ReceiverId, ReplicatedTransaction, ReplicationAnswer, RowChange, TransactionKey,
};
use fleet_sync_tokio_runner::WriterSessionCalls;
use fleet_sync_writer::{ReplicationTransport, SendFailure};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, oneshot};

struct StageGate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
pub(super) struct SplitTransport {
    inner: Arc<WriterSessionCalls<BoardProjectSchema, BoardConsumerProtocol>>,
    staged: AtomicUsize,
    staged_transactions: Mutex<Vec<TransactionKey>>,
    handoff_transactions: Mutex<Vec<TransactionKey>>,
    expansion_transactions: Mutex<Vec<TransactionKey>>,
    gate: Mutex<Option<StageGate>>,
}
impl SplitTransport {
    pub fn new(
        inner: Arc<WriterSessionCalls<BoardProjectSchema, BoardConsumerProtocol>>,
    ) -> (Arc<Self>, oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        (
            Arc::new(Self {
                inner,
                staged: AtomicUsize::new(0),
                staged_transactions: Mutex::new(Vec::new()),
                handoff_transactions: Mutex::new(Vec::new()),
                expansion_transactions: Mutex::new(Vec::new()),
                gate: Mutex::new(Some(StageGate {
                    entered,
                    release: released,
                })),
            }),
            observed,
            release,
        )
    }
    pub async fn held_transaction_wrote_expansion(&self) -> bool {
        let staged = self.staged_transactions.lock().await;
        let expansions = self.expansion_transactions.lock().await;
        staged
            .first()
            .is_some_and(|transaction| expansions.contains(transaction))
    }
    pub fn staged_answers(&self) -> usize {
        self.staged.load(Ordering::Acquire)
    }
    pub async fn held_transaction_wrote_handoff(&self) -> bool {
        let staged = self.staged_transactions.lock().await;
        let handoffs = self.handoff_transactions.lock().await;
        staged
            .first()
            .is_some_and(|transaction| handoffs.contains(transaction))
    }
}
impl ReplicationTransport<BoardConsumerProtocol> for SplitTransport {
    async fn send_piece(
        &self,
        copy: ReceiverId,
        piece: Piece<BoardAnnotation>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        // replicate_once applies the preceding answer before making this next
        // send, so the held point follows the writer's actual PartStaged path.
        if self.staged_answers() > 0
            && let Some(gate) = self.gate.lock().await.take()
        {
            let failed = || SendFailure::Unavailable {
                message: "split-copy fixture gate closed".to_owned(),
            };
            gate.entered.send(()).map_err(|_| failed())?;
            gate.release.await.map_err(|_| failed())?;
        }
        if let Some(part) = &piece.part
            && piece.transactions.iter().any(|transaction| match transaction {
                ReplicatedTransaction::Rows { rows, .. } => rows.iter().any(|row| matches!(row, RowChange::Upsert { table, .. } if table == "mail_handoffs")),
                ReplicatedTransaction::SchemaStep { .. } => false,
            })
        {
            self.handoff_transactions.lock().await.push(part.transaction);
        }
        if let Some(part) = &piece.part
            && piece.transactions.iter().any(|transaction| match transaction {
                ReplicatedTransaction::Rows { rows, .. } => rows.iter().any(|row| matches!(row, RowChange::Upsert { table, .. } if table == "mail_cancel_expansions")),
                ReplicatedTransaction::SchemaStep { .. } => false,
            }) {
            self.expansion_transactions.lock().await.push(part.transaction);
        }
        let answer = self.inner.send_piece(copy, piece).await?;
        if let ReplicationAnswer::PartStaged { transaction, .. } = &answer {
            self.staged_transactions.lock().await.push(*transaction);
            self.staged.fetch_add(1, Ordering::Release);
        }
        Ok(answer)
    }
}
