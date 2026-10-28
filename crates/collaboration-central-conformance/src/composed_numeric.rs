//! Exact scalar cells traverse real one-core sessions, snapshots and receipt replay.

use crate::{AdmittedPeerSocket, ConformanceClient, ConformanceResult, enroll_admitted_peer};
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, DatabaseId, Epoch, FrameId, OperationAnswer, OperationRequest,
    PeerId, Piece, PieceContents, ReplicatedTransaction, ReplicationAnswer, ReplicationPosition,
    ReplicationRequest, RowChange, ServerFrame, SnapshotCursor, SnapshotRead, SqlValue,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::Message;
use serde_json::json;
use std::time::Duration;

pub struct ComposedNumericScenario {
    pub(crate) database: DatabaseId,
    pub(crate) service: durable_steps::Uuid,
    pub(crate) peer: PeerId,
    pub(crate) credential: String,
    change: Piece<BoardAnnotation>,
    expected: Vec<Vec<SqlValue>>,
    real_key: String,
}

impl ComposedNumericScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        let service = durable_steps::Uuid::now_v7();
        let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
        let credential = format!("numeric-{service}");
        let mut socket = connect(client, service, peer, &credential).await?;
        let prepared = async {
            let created: serde_json::Value = client.with_owner_access(
                client.client.post(format!("{}/admin/v1/conformance/copy-call", client.base_url)),
            )?.json(&json!({
                "attempt":durable_steps::Uuid::now_v7(),
                "copy":"019f0000-0000-7000-8000-000000000010",
                "request":{"kind":"create","value":{
                    "database":database,"epoch":1,"holder":peer,"origin":{"kind":"empty"}
                }}
            })).send().await?.error_for_status()?.json().await?;
            if created != json!({"kind":"completed","value":{"kind":"created","value":fleet_sync_protocol::CreateCopyAnswer::CopyCreated}}) {
                return Err(format!("numeric copy was not created: {created}").into());
            }
            let schema = piece(database, 0, 1, vec![ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(1), migration_version: 1,
                statements: vec![
                    "CREATE TABLE scalar_records(record_key TEXT NOT NULL PRIMARY KEY,key_value,record_value)".into(),
                    "CREATE TABLE real_records(record_key TEXT NOT NULL PRIMARY KEY,key_value,record_value)".into(),
                ],
            }])?;
            applied(request(&mut socket, ReplicationRequest::Piece(schema)).await?, 1)?;

            // Literal old key/value oracle, now in value columns under UUIDv7 keys.
            let mut expected = Vec::new();
            let mut changes = Vec::new();
            for value in [i64::MIN, 9_007_199_254_740_992, 9_007_199_254_740_993, i64::MAX] {
                let key = durable_steps::Uuid::now_v7().to_string();
                let row = vec![SqlValue::Text(key.clone()), SqlValue::Integer(value), SqlValue::Integer(value)];
                changes.push(RowChange::upsert("scalar_records", vec![SqlValue::Text(key)], row.clone())?);
                expected.push(row);
            }
            let real_key = durable_steps::Uuid::now_v7().to_string();
            changes.push(RowChange::upsert("real_records", vec![SqlValue::Text(real_key.clone())], vec![
                SqlValue::Text(real_key.clone()), SqlValue::Integer(2), SqlValue::Real(1.0),
            ])?);
            let change = piece(database, 1, 2, vec![ReplicatedTransaction::Rows {
                commit_id: CommitSequence::new(2), rows: changes,
            }])?;
            applied(request(&mut socket, ReplicationRequest::Piece(change.clone())).await?, 2)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Self {
                database, service, peer, credential, change, expected, real_key,
            })
        }.await;
        let closed = socket
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        let prepared = prepared?;
        closed?;
        Ok(prepared)
    }

    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let mut socket = connect(client, self.service, self.peer, &self.credential).await?;
        let proof = async {
            let before = self.snapshot(&mut socket).await?;
            if request(&mut socket, ReplicationRequest::Piece(self.change.clone())).await?
                != (ReplicationAnswer::AlreadyApplied { through: position(2)? }) {
                return Err("numeric receipt did not replay the original acknowledged prefix".into());
            }
            let after = self.snapshot(&mut socket).await?;
            if before != after {
                return Err("numeric receipt replay changed the exact snapshot".into());
            }
            println!("COMPOSED_NUMERIC_CASE exact_i64_cells_real_identity_snapshot_and_receipt_replay passed");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        let closed = socket
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof?;
        closed?;
        Ok(())
    }

    async fn snapshot(
        &self,
        socket: &mut AdmittedPeerSocket,
    ) -> ConformanceResult<Vec<(String, Vec<Vec<SqlValue>>)>> {
        let mut cursor: Option<SnapshotCursor> = None;
        let mut pages = Vec::new();
        loop {
            let ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)) = request(
                socket,
                ReplicationRequest::SnapshotPage {
                    database: self.database,
                    cursor: cursor.clone(),
                },
            )
            .await?
            else {
                return Err("numeric snapshot did not return its real stored page".into());
            };
            if page.database != self.database || page.position != position(2)? {
                return Err("numeric snapshot lost its database or committed position".into());
            }
            let next = page.next;
            match page.table.as_str() {
                "scalar_records" if page.rows == self.expected => {}
                "real_records"
                    if page.rows
                        == vec![vec![
                            SqlValue::Text(self.real_key.clone()),
                            SqlValue::Integer(2),
                            SqlValue::Real(1.0),
                        ]] => {}
                _ => {
                    return Err(format!(
                        "numeric snapshot changed literal values or scalar tags: {:?}",
                        page.rows
                    )
                    .into());
                }
            }
            if pages.iter().any(|(table, _)| table == &page.table) {
                return Err("numeric snapshot repeated a table".into());
            }
            pages.push((page.table, page.rows));
            if next.is_none() {
                break;
            }
            if next == cursor {
                return Err("numeric snapshot cursor did not advance".into());
            }
            cursor = next;
        }
        if pages.len() != 2 {
            return Err("numeric snapshot omitted a scalar table".into());
        }
        Ok(pages)
    }
}

fn position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}
fn piece(
    database: DatabaseId,
    from: u64,
    through: u64,
    transactions: Vec<ReplicatedTransaction>,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    Ok(Piece::new(PieceContents {
        database,
        epoch: Epoch::new(1)?,
        continues_from: position(from)?,
        through: position(through)?,
        transactions,
        part: None,
        annotation: None,
    })?)
}
fn applied(answer: ReplicationAnswer, through: u64) -> ConformanceResult<()> {
    if answer
        != (ReplicationAnswer::Applied {
            through: position(through)?,
        })
    {
        return Err(format!("numeric piece was not applied: {answer:?}").into());
    }
    Ok(())
}
async fn connect(
    client: &ConformanceClient,
    service: durable_steps::Uuid,
    peer: PeerId,
    credential: &str,
) -> ConformanceResult<AdmittedPeerSocket> {
    enroll_admitted_peer(
        client,
        &service.to_string(),
        &peer.to_string(),
        credential,
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await
}
async fn request(
    socket: &mut AdmittedPeerSocket,
    body: ReplicationRequest<BoardAnnotation>,
) -> ConformanceResult<ReplicationAnswer> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Replication(body),
            })?
            .into(),
        ))
        .await?;
    let message = tokio::time::timeout(Duration::from_secs(10), socket.socket.next())
        .await?
        .ok_or("numeric socket ended")??;
    let bytes = match message {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => return Err(format!("numeric request received {other:?}").into()),
    };
    match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body: OperationAnswer::Replication(answer),
        } if actual == frame => Ok(answer),
        _ => Err("numeric request lost its frame correlation".into()),
    }
}
