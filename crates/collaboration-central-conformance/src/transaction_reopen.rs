//! Original rollback, fence, concurrent snapshot and reopen assertions on one core.

use crate::{
    AdmittedPeerSocket, ConformanceClient, ConformanceResult, RuntimeControl,
    composed_client::{control, copy_probe, operation},
    enroll_admitted_peer,
};
use board_sync_protocol::BoardAnnotation;
use fleet_sync_protocol::{
    CommitSequence, CopyOrigin, CopyStatusAnswer, CopyUnavailableReason, CreateCopy,
    CreateCopyAnswer, DatabaseId, Epoch, FenceExpectation, FenceRequest, FenceResult,
    OperationAnswer, OperationRequest, PeerId, Piece, PieceContents, PieceRefusal,
    ReplicatedTransaction, ReplicationAnswer, ReplicationPosition, ReplicationRequest, RowChange,
    SchemaSupport, SnapshotCursor, SnapshotRead, SqlValue, TransferId,
};
use reqwest_websocket::WebSocket;
use serde_json::json;
use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

struct CopyPeer {
    database: DatabaseId,
    socket: AdmittedPeerSocket,
}
struct PreparedCopy {
    database: DatabaseId,
    position: ReplicationPosition,
    rows: Vec<Vec<SqlValue>>,
    fence: Epoch,
}
pub struct ComposedTransactionScenario {
    copies: Vec<PreparedCopy>,
}
impl CopyPeer {
    async fn create(client: &ConformanceClient) -> ConformanceResult<Self> {
        let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
        let service = durable_steps::Uuid::now_v7();
        let socket = enroll_admitted_peer(
            client,
            &service.to_string(),
            &peer.to_string(),
            &format!("transaction-{service}"),
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        let created = copy_probe(
            client,
            json!({"kind":"create","value":CreateCopy {
                database, epoch: Epoch::new(1)?, holder: Some(peer), origin: CopyOrigin::Empty,
            }}),
        )
        .await?;
        if created != json!({"kind":"created","value":CreateCopyAnswer::CopyCreated}) {
            return Err("transaction copy was not created".into());
        }
        let mut fixture = Self { database, socket };
        let schema = piece(
            database,
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
        if fixture.apply(schema).await?
            != (ReplicationAnswer::Applied {
                through: position(1, 1)?,
            })
        {
            return Err("transaction schema was not applied".into());
        }
        Ok(fixture)
    }
    async fn apply(
        &mut self,
        piece: Piece<BoardAnnotation>,
    ) -> ConformanceResult<ReplicationAnswer> {
        replication(&mut self.socket.socket, ReplicationRequest::Piece(piece)).await
    }
    fn address(&self) -> ConformanceResult<collaboration_central_core::ObjectAddress> {
        Ok(collaboration_central_core::ObjectAddress::Copy(
            fleet_sync_protocol::DatabaseId::try_from(self.database.to_string())?,
        ))
    }
    async fn close(self) -> ConformanceResult<()> {
        self.socket
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
}
impl ComposedTransactionScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let mut sql = CopyPeer::create(client).await?;
        let mut full = CopyPeer::create(client).await?;
        let mut fenced = CopyPeer::create(client).await?;
        let mut concurrent = CopyPeer::create(client).await?;
        let proof = async {
            let invalid = row_piece(sql.database, position(1,1)?, position(1,2)?, vec![(1,"same".to_owned()),(2,"same".to_owned())])?;
            if !matches!(sql.apply(invalid).await?, ReplicationAnswer::CopyUnavailable { reason: CopyUnavailableReason::Damaged { detail } } if detail.to_ascii_lowercase().contains("unique")) {
                return Err("the late actual SQL uniqueness failure was not reached".into());
            }
            control(client, full.address()?, RuntimeControl::CopyFull { after_writes: NonZeroU64::new(2).ok_or("positive threshold")? }).await?;
            let offered = row_piece(full.database, position(1,1)?, position(1,2)?, vec![(1,"first".to_owned()),(2,"second".to_owned())])?;
            if full.apply(offered).await? != (ReplicationAnswer::CopyUnavailable { reason: CopyUnavailableReason::StorageFull }) { return Err("the real late callback full failure was not preserved".into()); }

            let holder = PeerId::new(durable_steps::Uuid::parse_str(&fenced.socket.peer.to_string())?)?;
            let fence = FenceRequest {
                database: fenced.database, transfer_id: TransferId::new(durable_steps::Uuid::now_v7())?, new_epoch: Epoch::new(2)?,
                expect: FenceExpectation { epoch: Epoch::new(1)?, acked: position(1,1)? },
                target_schema: Some(SchemaSupport { max_version: 10 }), holder,
            };
            let answer = copy_probe(client, json!({"kind":"fence","value":fence})).await?;
            if answer != json!({"kind":"fenced","value":FenceResult::Fenced { old_epoch: Epoch::new(1)?, new_epoch: Epoch::new(2)?, start: position(1,1)? }}) { return Err("copy was not fenced at its exact prefix".into()); }
            let stale = row_piece(fenced.database, position(1,1)?, position(1,2)?, vec![(1,"stale".to_owned())])?;
            if fenced.apply(stale).await? != (ReplicationAnswer::Refused(PieceRefusal::StaleEpoch { newer: Epoch::new(2)?, ended_at: position(1,1)? })) { return Err("the old epoch was not refused at its immutable cutoff".into()); }

            let expected = (0..300).map(|ordinal| vec![SqlValue::Text(row_key(ordinal)), SqlValue::Text(format!("row-{ordinal}"))]).collect::<Vec<_>>();
            let offered = row_piece(concurrent.database, position(1,1)?, position(1,2)?, (0..300).map(|ordinal|(ordinal,format!("row-{ordinal}"))).collect())?;
            let mut observer = observer(client).await?;
            let finished = AtomicBool::new(false);
            let database = concurrent.database;
            let (applied, observed) = tokio::join!(async {
                let applied = concurrent.apply(offered).await;
                finished.store(true, Ordering::SeqCst);
                applied
            }, async {
                tokio::time::timeout(Duration::from_secs(20), async {
                    loop {
                        let finished_before_read = finished.load(Ordering::SeqCst);
                        let actual = snapshot(&mut observer.socket, database).await?;
                        if actual.position == position(1,1)? && actual.rows.is_empty() {
                            if finished_before_read {
                                return Err("a read after the acknowledgement still sees the old position".into());
                            }
                            continue;
                        }
                        if actual.position == position(1,2)? && actual.rows == expected && actual.page_sizes == vec![256,44] {
                            return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                        }
                        return Err("concurrent snapshot exposed a partial committed prefix".into());
                    }
                }).await?
            });
            let closed = observer.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
            if applied? != (ReplicationAnswer::Applied { through: position(1,2)? }) { return Err("the complete 300-row transaction was not acknowledged".into()); }
            observed?; closed?;
            let mut scenario = Self { copies: vec![
                PreparedCopy { database: sql.database, position: position(1,1)?, rows: Vec::new(), fence: Epoch::new(1)? },
                PreparedCopy { database: full.database, position: position(1,1)?, rows: Vec::new(), fence: Epoch::new(1)? },
                PreparedCopy { database: fenced.database, position: position(1,1)?, rows: Vec::new(), fence: Epoch::new(2)? },
                PreparedCopy { database: concurrent.database, position: position(1,2)?, rows: expected, fence: Epoch::new(1)? },
            ] };
            scenario.verify(client).await?;
            let matching = row_piece(fenced.database, position(1,1)?, position(2,1)?, vec![(1,"matching".to_owned())])?;
            if fenced.apply(matching).await? != (ReplicationAnswer::Applied { through: position(2,1)? }) { return Err("matching new epoch did not apply its one complete row".into()); }
            let saved = scenario.copies.get_mut(2).ok_or("fenced copy fixture")?;
            saved.position = position(2,1)?;
            saved.rows = vec![vec![SqlValue::Text(row_key(1)),SqlValue::Text("matching".into())]];
            scenario.verify(client).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(scenario)
        }.await;
        let closed_sql = sql.close().await;
        let closed_full = full.close().await;
        let closed_fenced = fenced.close().await;
        let closed_concurrent = concurrent.close().await;
        let proof = proof?;
        closed_sql?;
        closed_full?;
        closed_fenced?;
        closed_concurrent?;
        Ok(proof)
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let mut observer = observer(client).await?;
        let proof = async {
            for expected in &self.copies {
                let actual = snapshot(&mut observer.socket, expected.database).await?;
                if actual.position != expected.position || actual.rows != expected.rows { return Err("owning reopen changed rows or their exact acknowledgement".into()); }
                let ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)) = replication(&mut observer.socket, ReplicationRequest::CopyStatus { database: expected.database }).await? else { return Err("reopened copy status absent".into()); };
                if status.fence != expected.fence || status.acked != expected.position { return Err("reopened copy lost its durable fence or acknowledgement".into()); }
            }
            println!("COMPOSED_TRANSACTION_CASE sql_and_full_rollbacks_fence_300_row_snapshot_prefixes_and_reopen passed");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        let closed = observer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof?;
        closed?;
        Ok(())
    }
}
struct CompleteSnapshot {
    position: ReplicationPosition,
    rows: Vec<Vec<SqlValue>>,
    page_sizes: Vec<usize>,
}
async fn snapshot(
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<CompleteSnapshot> {
    tokio::time::timeout(Duration::from_secs(20), async {
        'restart: loop {
            let mut cursor: Option<SnapshotCursor> = None;
            let mut captured = None;
            let mut rows = Vec::new();
            let mut page_sizes = Vec::new();
            loop {
                match replication(
                    socket,
                    ReplicationRequest::SnapshotPage {
                        database,
                        cursor: cursor.clone(),
                    },
                )
                .await?
                {
                    ReplicationAnswer::SnapshotPage(SnapshotRead::Moved { .. }) => {
                        continue 'restart;
                    }
                    ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))
                        if page.database == database && page.table == "records" =>
                    {
                        if captured.is_some_and(|position| position != page.position) {
                            return Err("snapshot pages mixed committed positions".into());
                        }
                        captured = Some(page.position);
                        page_sizes.push(page.rows.len());
                        rows.extend(page.rows);
                        if page.next.is_none() {
                            return Ok(CompleteSnapshot {
                                position: page.position,
                                rows,
                                page_sizes,
                            });
                        }
                        if page.next == cursor {
                            return Err("snapshot cursor failed to advance".into());
                        }
                        cursor = page.next;
                    }
                    _ => return Err("snapshot did not return the original records table".into()),
                }
            }
        }
    })
    .await?
}
async fn replication(
    socket: &mut WebSocket,
    request: ReplicationRequest<BoardAnnotation>,
) -> ConformanceResult<ReplicationAnswer> {
    match operation(socket, OperationRequest::Replication(request)).await? {
        OperationAnswer::Replication(answer) => Ok(answer),
        _ => Err("transaction operation changed its answer family".into()),
    }
}
async fn observer(client: &ConformanceClient) -> ConformanceResult<AdmittedPeerSocket> {
    let service = durable_steps::Uuid::now_v7();
    enroll_admitted_peer(
        client,
        &service.to_string(),
        &durable_steps::Uuid::now_v7().to_string(),
        &format!("snapshot-{service}"),
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await
}
fn row_key(ordinal: u64) -> String {
    format!("019f0000-0000-7000-8000-{ordinal:012x}")
}
fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
fn row_piece(
    database: DatabaseId,
    from: ReplicationPosition,
    through: ReplicationPosition,
    rows: Vec<(u64, String)>,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    let rows = rows
        .into_iter()
        .map(|(ordinal, label)| {
            RowChange::upsert(
                "records",
                vec![SqlValue::Text(row_key(ordinal))],
                vec![SqlValue::Text(row_key(ordinal)), SqlValue::Text(label)],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    piece(
        database,
        from,
        through,
        vec![ReplicatedTransaction::Rows {
            commit_id: through.sequence,
            rows,
        }],
    )
}
fn piece(
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
