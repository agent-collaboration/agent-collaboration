//! Real session reads retain shared snapshot answers and per-copy containment.

use super::copy_wait_scenarios::CopyClient;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{
    ClientFrame, CopyStatusAnswer, CopyUnavailableReason, DatabaseId, Epoch, FrameId, NoConsumer,
    OperationAnswer, OperationRequest, PieceRefusal, ReplicationAnswer, ReplicationRequest,
    ServerFrame, SnapshotCursor, SnapshotManifestAnswer, SnapshotRead,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

pub async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut peer = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-4466554400e3",
        "019f0000-0000-7000-8000-000000001020",
        "copy-read-session",
        "019f0000-0000-7000-8000-000000000941",
    )
    .await?;
    let result = async {
        let absent = database("942")?;
        if !matches!(request(&mut peer.socket, ReplicationRequest::CopyStatus { database: absent }).await?,
            ReplicationAnswer::CopyUnavailable { reason: CopyUnavailableReason::Uninitialized }) {
            return Err("uninitialized copy did not give its typed per-operation refusal".into());
        }
        let copies = CopyClient::new(base)?;
        let retired = database("943")?;
        let healthy = database("944")?;
        copies.create(retired).await?;
        copies.create(healthy).await?;
        let status = request(&mut peer.socket, ReplicationRequest::CopyStatus { database: retired }).await?;
        if !matches!(status, ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(ref copy))
            if copy.database == retired && copy.acked == super::copy_wait_scenarios::position(0)?) {
            return Err("same session did not read the real initialized copy".into());
        }
        page(&mut peer.socket, retired).await?;
        let manifest = request(&mut peer.socket, ReplicationRequest::SnapshotManifest {
            database: retired, position: super::copy_wait_scenarios::position(0)?,
        }).await?;
        if !matches!(manifest, ReplicationAnswer::SnapshotManifest(SnapshotManifestAnswer::Manifest(ref manifest))
            if manifest.database == retired && manifest.position == super::copy_wait_scenarios::position(0)?
                && manifest.schema.version == 0 && manifest.tables.is_empty()) {
            return Err("snapshot manifest did not retain the real copy identity, position and schema".into());
        }
        let moved = request(&mut peer.socket, ReplicationRequest::SnapshotPage {
            database: retired,
            cursor: Some(SnapshotCursor {
                position: super::copy_wait_scenarios::position(1)?,
                table: "unused-at-an-old-position".into(), after_key: Vec::new(),
            }),
        }).await?;
        if !matches!(moved, ReplicationAnswer::SnapshotPage(SnapshotRead::Moved { position })
            if position == super::copy_wait_scenarios::position(0)?) {
            return Err("snapshot cursor did not return its real moved position".into());
        }
        copies.retire(retired, Epoch::new(1)?).await?;
        if !matches!(request(&mut peer.socket, ReplicationRequest::SnapshotPage { database: retired, cursor: None }).await?,
            ReplicationAnswer::Refused(PieceRefusal::Retired)) {
            return Err("retired snapshot did not preserve its typed rollback refusal".into());
        }
        if !matches!(request(&mut peer.socket, ReplicationRequest::SnapshotManifest {
            database: retired, position: super::copy_wait_scenarios::position(0)?,
        }).await?, ReplicationAnswer::SnapshotManifest(SnapshotManifestAnswer::Retired)) {
            return Err("retired manifest did not preserve the shared read answer".into());
        }
        page(&mut peer.socket, healthy).await?;
        println!("COPY_READ_CASE uninitialized_real_snapshot_moved_cursor_and_retired_copy_keep_the_same_other_project_session_usable passed");
        Ok::<_, Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}
fn database(suffix: &str) -> ConformanceResult<DatabaseId> {
    Ok(DatabaseId::try_from(format!(
        "019f0000-0000-7000-8000-000000000{suffix}"
    ))?)
}
pub(super) async fn page(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    let answer = request(
        socket,
        ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        },
    )
    .await?;
    let ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)) = answer else {
        return Err("snapshot read did not return its shared page".into());
    };
    if page.database != database
        || page.position != super::copy_wait_scenarios::position(0)?
        || page.schema.version != 0
        || !page.schema_steps.is_empty()
        || !page.table.is_empty()
        || !page.rows.is_empty()
        || page.next.is_some()
    {
        return Err(
            "empty copy snapshot did not preserve exact identity, position, schema and page bounds"
                .into(),
        );
    }
    Ok(())
}
pub(super) async fn request(
    socket: &mut WebSocket,
    request: ReplicationRequest,
) -> ConformanceResult<ReplicationAnswer> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<NoConsumer>::Request {
                frame,
                body: OperationRequest::Replication(request),
            })?
            .into(),
        ))
        .await?;
    let message = tokio::time::timeout(Duration::from_secs(8), socket.next())
        .await?
        .ok_or("copy read lost its healthy socket")??;
    let bytes = match message {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => {
            return Err(format!(
                "copy read received a physical close instead of a typed answer: {other:?}"
            )
            .into());
        }
    };
    match serde_json::from_slice::<ServerFrame<NoConsumer>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body: OperationAnswer::Replication(answer),
        } if actual == frame => Ok(answer),
        _ => Err("copy read lost its frame correlation or replication answer family".into()),
    }
}
