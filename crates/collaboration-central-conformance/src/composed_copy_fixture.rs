//! Copy scenarios use real admitted sessions and the composed Registry journal.
use crate::{
    ConformanceClient, ConformanceResult,
    composed_client::{copy_probe, operation},
    enroll_admitted_peer,
};
use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{
    CommitSequence, CopyOrigin, CopyStatus, CopyStatusAnswer, CreateCopy, CreateCopyAnswer,
    DatabaseId, Epoch, FenceRequest, FenceResult, OperationAnswer, OperationRequest, PeerId, Piece,
    PieceContents, PieceDigest, ReplicatedTransaction, ReplicationAnswer, ReplicationPosition,
    ReplicationRequest, RowChange, SnapshotCursor, SnapshotPage, SnapshotRead, SqlValue,
    TransactionKey, TransactionPart,
};
use serde_json::json;

pub(crate) struct CopyFixture {
    pub database: DatabaseId,
    pub peer: PeerId,
    service: durable_steps::Uuid,
    credential: String,
}
impl CopyFixture {
    pub async fn create(client: &ConformanceClient) -> ConformanceResult<Self> {
        let fixture = Self {
            database: DatabaseId::new(durable_steps::Uuid::now_v7())?,
            peer: PeerId::new(durable_steps::Uuid::now_v7())?,
            service: durable_steps::Uuid::now_v7(),
            credential: format!("copy-suite-{}", durable_steps::Uuid::now_v7()),
        };
        let created = copy_probe(client, json!({"kind":"create","value":CreateCopy {
            database:fixture.database, epoch:Epoch::new(1)?, holder:Some(fixture.peer), origin:CopyOrigin::Empty,
        }})).await?;
        if created != json!({"kind":"created","value":CreateCopyAnswer::CopyCreated}) {
            return Err("suite copy was not created".into());
        }
        let schema = piece(
            fixture.database,
            position(1, 0)?,
            position(1, 1)?,
            vec![ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(1),
                migration_version: 1,
                statements: vec![
                    "CREATE TABLE records(id TEXT NOT NULL PRIMARY KEY,label TEXT NOT NULL UNIQUE)"
                        .into(),
                ],
            }],
        )?;
        equal(
            fixture.apply(client, schema).await?,
            ReplicationAnswer::Applied {
                through: position(1, 1)?,
            },
        )?;
        Ok(fixture)
    }
    pub fn address(&self) -> ConformanceResult<collaboration_central_core::ObjectAddress> {
        Ok(collaboration_central_core::ObjectAddress::Copy(
            fleet_sync_protocol::DatabaseId::try_from(self.database.to_string())?,
        ))
    }
    pub async fn request(
        &self,
        client: &ConformanceClient,
        request: ReplicationRequest<BoardAnnotation>,
    ) -> ConformanceResult<ReplicationAnswer> {
        let mut peer = enroll_admitted_peer(
            client,
            &self.service.to_string(),
            &self.peer.to_string(),
            &self.credential,
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        let answer = operation(&mut peer.socket, OperationRequest::Replication(request)).await;
        let closed = peer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let answer = answer?;
        closed?;
        match answer {
            OperationAnswer::Replication(answer) => Ok(answer),
            _ => Err("copy suite changed answer family".into()),
        }
    }
    pub async fn apply(
        &self,
        client: &ConformanceClient,
        piece: Piece<BoardAnnotation>,
    ) -> ConformanceResult<ReplicationAnswer> {
        self.request(client, ReplicationRequest::Piece(piece)).await
    }
    pub async fn status(&self, client: &ConformanceClient) -> ConformanceResult<CopyStatus> {
        match self
            .request(
                client,
                ReplicationRequest::CopyStatus {
                    database: self.database,
                },
            )
            .await?
        {
            ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)) => Ok(status),
            other => Err(format!("copy suite status {other:?}").into()),
        }
    }
    pub async fn page(
        &self,
        client: &ConformanceClient,
        cursor: Option<SnapshotCursor>,
    ) -> ConformanceResult<SnapshotRead> {
        match self
            .request(
                client,
                ReplicationRequest::SnapshotPage {
                    database: self.database,
                    cursor,
                },
            )
            .await?
        {
            ReplicationAnswer::SnapshotPage(page) => Ok(page),
            other => Err(format!("copy suite snapshot {other:?}").into()),
        }
    }
    pub async fn first_page(&self, client: &ConformanceClient) -> ConformanceResult<SnapshotPage> {
        match self.page(client, None).await? {
            SnapshotRead::Page(page) => Ok(*page),
            other @ SnapshotRead::Moved { .. } => {
                Err(format!("copy suite first page {other:?}").into())
            }
        }
    }
    pub async fn fence(
        &self,
        client: &ConformanceClient,
        request: FenceRequest,
    ) -> ConformanceResult<FenceResult> {
        let value = copy_probe(client, json!({"kind":"fence","value":request})).await?;
        if value.get("kind").and_then(serde_json::Value::as_str) != Some("fenced") {
            return Err(format!("copy suite fence {value}").into());
        }
        Ok(serde_json::from_value(
            value.get("value").cloned().ok_or("missing fence value")?,
        )?)
    }
}
pub(crate) fn equal<T: PartialEq + std::fmt::Debug>(
    actual: T,
    expected: T,
) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("copy suite expected {expected:?}, got {actual:?}").into());
    }
    Ok(())
}
pub(crate) fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
pub(crate) fn row_key(ordinal: u64) -> String {
    format!("019f0000-0000-7000-8000-{ordinal:012x}")
}
pub(crate) fn row(ordinal: u64, label: &str) -> ConformanceResult<RowChange> {
    Ok(RowChange::upsert(
        "records",
        vec![SqlValue::Text(row_key(ordinal))],
        vec![
            SqlValue::Text(row_key(ordinal)),
            SqlValue::Text(label.into()),
        ],
    )?)
}
pub(crate) fn rows(commit: u64, rows: Vec<RowChange>) -> ReplicatedTransaction {
    ReplicatedTransaction::Rows {
        commit_id: CommitSequence::new(commit),
        rows,
    }
}
pub(crate) fn piece(
    database: DatabaseId,
    from: ReplicationPosition,
    through: ReplicationPosition,
    transactions: Vec<ReplicatedTransaction>,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    Ok(Piece::new(PieceContents {
        database,
        epoch: through.epoch,
        continues_from: from,
        through,
        transactions,
        part: None,
        annotation: None,
    })?)
}
pub(crate) fn parts(
    database: DatabaseId,
    count: u32,
) -> ConformanceResult<Vec<Piece<BoardAnnotation>>> {
    let all = rows(
        4,
        (0..count)
            .map(|index| row(u64::from(index), &format!("part-{index}")))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let digest = PieceDigest::of_transaction(&all)?;
    (0..count)
        .map(|index| {
            Ok(Piece::new(PieceContents {
                database,
                epoch: Epoch::new(1)?,
                continues_from: position(1, 1)?,
                through: position(1, 4)?,
                transactions: vec![rows(
                    4,
                    vec![row(u64::from(index), &format!("part-{index}"))?],
                )],
                part: Some(TransactionPart::new(
                    TransactionKey {
                        epoch: Epoch::new(1)?,
                        commit_id: CommitSequence::new(4),
                    },
                    index,
                    count,
                    digest.clone(),
                )?),
                annotation: None,
            })?)
        })
        .collect()
}
