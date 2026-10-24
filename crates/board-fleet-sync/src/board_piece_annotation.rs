//! Summaries describe the piece's highest feed position, including no-status projects.
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use fleet_sync_writer::{CapturedTransaction, PieceAnnotation};
use message_board_storage::{CapturedFeedSummary, CapturedProjectChanges};

pub struct BoardPieceAnnotation;

impl PieceAnnotation for BoardPieceAnnotation {
    type Protocol = BoardConsumerProtocol;

    fn annotate(&self, transactions: &[CapturedTransaction<'_>]) -> Option<BoardAnnotation> {
        // The sender runs fallible BoardCaptureBookkeeping before annotation.
        let latest =
            transactions
                .iter()
                .try_fold(None::<CapturedFeedSummary>, |latest, transaction| {
                    let next = CapturedProjectChanges::decode(transaction)?.latest_feed;
                    Ok::<_, message_board_storage::ProjectStorageError>(match (latest, next) {
                        (Some(previous), Some(next))
                            if previous.cursor.position > next.cursor.position =>
                        {
                            Some(previous)
                        }
                        (previous, None) => previous,
                        (_, next) => next,
                    })
                });
        match latest {
            Ok(Some(latest)) => Some(BoardAnnotation::ProjectSummary {
                status: latest.status,
                latest_record_at: latest.time,
            }),
            Ok(None) => None,
            Err(error) => {
                tracing::error!(%error, "captured board row failed annotation validation");
                None
            }
        }
    }
}
