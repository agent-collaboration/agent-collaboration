//! Explicit board-policy read input for the generic FleetService component fixture.
use board_sync_protocol::{
    BoardPayload, BoardReadCursor, BoardReadPage, BoardReadScope, FleetFeedRead,
};
use fleet_sync_service::ServiceError;
use message_board::FleetPosition;
use message_board_storage::FleetRecordSnapshot;
use tokio::sync::{Mutex, oneshot};

struct FrozenFleet {
    through: FleetPosition,
    records: Vec<BoardPayload>,
}
#[derive(Default)]
pub(super) struct FleetReadProbe {
    frozen: Mutex<Option<FrozenFleet>>,
    held_reply: Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>,
}
impl FleetReadProbe {
    pub async fn hold_next_reply(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, waiting) = oneshot::channel();
        let (release, released) = oneshot::channel();
        *self.held_reply.lock().await = Some((entered, released));
        (waiting, release)
    }

    pub async fn freeze(
        &self,
        snapshot: FleetRecordSnapshot,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Fleet snapshots page by (record id, kind); cache discovery reads use
        // another ordering. Preserve the public continuation contract here.
        let mut rows = snapshot
            .records
            .iter()
            .map(|record| {
                let reference = record.reference();
                Ok((
                    (
                        reference.id.as_str().to_owned(),
                        serde_json::to_string(&reference.kind)?,
                    ),
                    BoardPayload::encode(record)?,
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        let records = rows.into_iter().map(|(_, payload)| payload).collect();
        *self.frozen.lock().await = Some(FrozenFleet {
            through: snapshot.through,
            records,
        });
        Ok(())
    }
    pub async fn read(
        &self,
        scope: &BoardReadScope,
        cursor: Option<BoardReadCursor>,
    ) -> Result<BoardReadPage, ServiceError> {
        let answer = {
            let frozen = self.frozen.lock().await;
            let Some(frozen) = frozen.as_ref() else {
                return Ok(BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid));
            };
            let invalid = || BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid);
            match (scope, cursor) {
                (BoardReadScope::FleetRecords, Some(BoardReadCursor::Fleet(after)))
                    if after == frozen.through =>
                {
                    BoardReadPage::FleetFeed(FleetFeedRead::Page {
                        records: Vec::new(),
                        through: frozen.through,
                        head: frozen.through,
                        as_of: chrono::Utc::now(),
                    })
                }
                (BoardReadScope::FleetRecords, Some(BoardReadCursor::Fleet(after)))
                    if after < frozen.through =>
                {
                    BoardReadPage::FleetFeed(FleetFeedRead::CursorExpired {
                        floor: frozen.through,
                    })
                }
                (BoardReadScope::FleetSnapshot, None) => BoardReadPage::FleetSnapshot {
                    records: frozen.records.clone(),
                    next: None,
                    snapshot_at: frozen.through,
                    as_of: chrono::Utc::now(),
                },
                _ => invalid(),
            }
        };
        if let Some((entered, released)) = self.held_reply.lock().await.take() {
            let _ = entered.send(());
            released.await.map_err(|error| ServiceError::Rule {
                message: error.to_string(),
            })?;
        }
        Ok(answer)
    }
}
