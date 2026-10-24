//! State carried from the complete old prepare body to its surviving/reopened reader.
use super::*;
#[path = "preludes.rs"]
mod preludes;
#[path = "prepare.rs"]
mod prepare;
#[path = "reopen.rs"]
mod reopen;
#[path = "verify.rs"]
mod verify;
pub(super) use preludes::run as run_preludes;

const OLD_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440821";
const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000821";
const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440822";
const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000000822";
const OLD_CREDENTIAL: &str = "normal redeploy old holder";
const TARGET_CREDENTIAL: &str = "normal redeploy target";

pub(super) fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
fn row(id: i64, label: &str) -> ConformanceResult<RowChange> {
    Ok(RowChange::upsert(
        "records",
        vec![SqlValue::Text(
            registry_setup::normal_record_key(id)?.into(),
        )],
        vec![
            SqlValue::Text(registry_setup::normal_record_key(id)?.into()),
            SqlValue::Integer(id),
            SqlValue::Text(label.into()),
        ],
    )?)
}
fn delete(id: i64) -> ConformanceResult<RowChange> {
    Ok(RowChange::delete(
        "records",
        vec![SqlValue::Text(
            registry_setup::normal_record_key(id)?.into(),
        )],
    )?)
}
fn piece(
    database: DatabaseId,
    from: ReplicationPosition,
    through: ReplicationPosition,
    rows: Vec<RowChange>,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    Ok(Piece::new(PieceContents {
        database,
        epoch: through.epoch,
        continues_from: from,
        through,
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: through.sequence,
            rows,
        }],
        part: None,
        annotation: None,
    })?)
}
pub(super) struct NormalRedeployState {
    project: DatabaseId,
    takeover: Value,
    stale: Piece<BoardAnnotation>,
    in_flight: Piece<BoardAnnotation>,
    in_flight_frame: FrameId,
    original_target: WebSocket,
    old_session: SessionId,
    target_session: SessionId,
}
pub(super) struct ClosedNormalRedeployState {
    state: NormalRedeployState,
}
impl NormalRedeployState {
    pub(super) async fn close(self) -> ConformanceResult<()> {
        let _ = self
            .original_target
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        Ok(())
    }
}
