//! Snapshot cuts survive writes and server restart; later feed entries close the cut.
use super::feed_reads::{accepted, create, offered};
use super::*;
use board_sync_protocol::{
    BoardReadCursor, BoardReadPage, BoardReadScope, FleetFeedRead, FleetSnapshotCursor,
};
use message_board::{FleetPosition, WorkspaceId};
use std::collections::BTreeSet;

const EARLY_WORKSPACE: &str = "00000000-0000-7000-8000-000000000001";
const LATE_WORKSPACE: &str = "ffffffff-ffff-7fff-8fff-fffffffffff1";

async fn read(
    socket: &mut WebSocket,
    scope: BoardReadScope,
    cursor: Option<BoardReadCursor>,
) -> ConformanceResult<BoardReadPage> {
    match operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read { scope, cursor }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(page)) => Ok(page),
        _ => Err("snapshot read returned a different operation family".into()),
    }
}
fn update(name: &str) -> ConformanceResult<FleetRequest> {
    let mut update = offered(FleetOperation::WorkspaceUpdate {
        name: message_board::ResourceName::try_from(name.to_owned())?,
        description: message_board::Description::try_from("Snapshot interleaving".to_owned())?,
    })?;
    update.workspace = WorkspaceId::try_from(LATE_WORKSPACE.to_owned())?;
    Ok(update)
}
fn records(payloads: Vec<BoardPayload>) -> ConformanceResult<Vec<Record>> {
    payloads
        .into_iter()
        .map(|payload| payload.decode().map_err(Into::into))
        .collect()
}
fn ordered(records: &[Record]) -> ConformanceResult<()> {
    let keys = records
        .iter()
        .map(|record| {
            let reference = record.reference();
            Ok((
                reference.id.as_str().to_owned(),
                serde_json::to_value(reference.kind)?
                    .as_str()
                    .ok_or("kind is not a string")?
                    .to_owned(),
            ))
        })
        .collect::<ConformanceResult<Vec<_>>>()?;
    if keys
        .iter()
        .zip(keys.iter().skip(1))
        .any(|(left, right)| left >= right)
    {
        return Err("snapshot keys are not in RecordId/kind order".into());
    }
    Ok(())
}

#[tokio::test]
async fn normal_snapshot_keeps_its_cut_across_reopen_and_current_rows_are_repaired_by_the_feed()
-> ConformanceResult<()> {
    // No pre-fix RED was observed for this snapshot scenario. It exercises the
    // real Session/Fleet path; Router replacement-cache visibility is separate.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let first = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let setup=async {
        let client=ConformanceClient::new(first.base_url())?;
        let mut peer=enroll_admitted_peer(&client,SERVICE,"019f0000-0000-7000-8000-00000000100f","snapshot-origin","019f0000-0000-7000-8000-000000000f47").await?;
        let setup=async {
            match read(&mut peer.socket,BoardReadScope::FleetSnapshot,None).await? {
                BoardReadPage::FleetSnapshot {records,next:None,snapshot_at,..} if records.is_empty() && snapshot_at==FleetPosition::ZERO=>{}
                _=>return Err("empty Fleet snapshot was not explicitly complete at zero".into()),
            }
            let mut late=create()?;
            late.workspace=WorkspaceId::try_from(LATE_WORKSPACE.to_owned())?;
            accepted(&mut peer.socket,&late).await?;
            let mut early=create()?;
            early.workspace=WorkspaceId::try_from(EARLY_WORKSPACE.to_owned())?;
            early.operation=FleetOperation::WorkspaceCreate {
                name:message_board::ResourceName::try_from("Early key in snapshot".to_owned())?,
                description:message_board::Description::try_from("Record kind must not outrank UUID identity".to_owned())?,
            };
            accepted(&mut peer.socket,&early).await?;
            for number in 1..=254 {
                accepted(&mut peer.socket,&update(&format!("Snapshot revision {number}"))?).await?;
            }
            for (after,expected_through) in [(0,256),(256,512)] {
                match read(&mut peer.socket,BoardReadScope::FleetRecords,Some(BoardReadCursor::Fleet(FleetPosition::try_from(after)?))).await? {
                    BoardReadPage::FleetFeed(FleetFeedRead::Page {records,through,head,..}) if records.len()==256 && through.get()==expected_through && head.get()==512=>{},
                    _=>return Err("large feed did not expose its bounded contiguous prefix and current head".into()),
                }
            }
            let BoardReadPage::FleetSnapshot {records:payloads,next:Some(next),snapshot_at,..}=
                read(&mut peer.socket,BoardReadScope::FleetSnapshot,None).await? else {
                return Err("large Fleet snapshot was not paged".into());
            };
            let records=records(payloads)?;
            ordered(&records)?;
            if records.len()!=256 || snapshot_at.get()!=512 || next.snapshot_at!=snapshot_at
                || records.last().map(Record::reference)!=Some(next.after.clone())
                || !matches!(records.first(),Some(Record::Workspace(workspace)) if workspace.workspace_id.as_str()==EARLY_WORKSPACE) {
                return Err("snapshot page lost its bound, identity order or frozen head".into());
            }
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>((records,next,snapshot_at))
        }.await;
        let _=peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        setup
    }.await;
    first.shutdown().await?;
    let (first_records, cursor, snapshot_at) = setup?;
    let second = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let proof=async {
        let client=ConformanceClient::new(second.base_url())?;
        let mut peer=enroll_admitted_peer(&client,SERVICE,"019f0000-0000-7000-8000-00000000100f","snapshot-origin","019f0000-0000-7000-8000-000000000f48").await?;
        let proof=async {
            accepted(&mut peer.socket,&update("Between snapshot pages")?).await?;
            accepted(&mut peer.socket,&update("Newest snapshot state")?).await?;
            let BoardReadPage::FleetSnapshot {records:payloads,next:None,snapshot_at:continued,..}=
                read(&mut peer.socket,BoardReadScope::FleetSnapshot,Some(BoardReadCursor::FleetSnapshot(cursor.clone()))).await? else {
                return Err("valid continuation did not finish after server reopen".into());
            };
            let continued_records=records(payloads)?;
            ordered(&continued_records)?;
            if continued!=snapshot_at || continued_records.len()!=4
                || !matches!(continued_records.last(),Some(Record::Workspace(workspace)) if workspace.workspace_id.as_str()==LATE_WORKSPACE && workspace.name.as_str()=="Newest snapshot state" && workspace.envelope.position.fleet_position().is_some_and(|position|position.get()==515)) {
                return Err("snapshot continuation changed its cut or lost current-row semantics".into());
            }
            let identities=first_records.iter().chain(continued_records.iter()).map(|record| {
                let reference=record.reference();
                Ok((reference.id.as_str().to_owned(),serde_json::to_string(&reference.kind)?))
            }).collect::<ConformanceResult<BTreeSet<_>>>()?;
            if identities.len()!=260 {return Err("snapshot traversal lost or repeated a stable identity".into());}
            let BoardReadPage::FleetFeed(FleetFeedRead::Page {records:payloads,through,head,..})=
                read(&mut peer.socket,BoardReadScope::FleetRecords,Some(BoardReadCursor::Fleet(snapshot_at))).await? else {
                return Err("snapshot cut could not resume the feed".into());
            };
            let catchup=records(payloads)?;
            if through.get()!=516 || through!=head || catchup.len()!=4
                || !matches!(catchup.first(),Some(Record::Workspace(workspace)) if workspace.name.as_str()=="Between snapshot pages") {
                return Err("feed did not include the genuine older-then-newer catch-up interleaving".into());
            }
            if !matches!(catchup.iter().rev().find(|record|matches!(record,Record::Workspace(workspace) if workspace.workspace_id.as_str()==LATE_WORKSPACE)),Some(Record::Workspace(workspace)) if workspace.name.as_str()=="Newest snapshot state") {
                return Err("feed catch-up did not end with the latest workspace state".into());
            }
            let future=FleetSnapshotCursor {snapshot_at:FleetPosition::try_from(517)?,after:cursor.after};
            if !matches!(read(&mut peer.socket,BoardReadScope::FleetSnapshot,Some(BoardReadCursor::FleetSnapshot(future))).await?,BoardReadPage::FleetFeed(FleetFeedRead::CursorInvalid)) {
                return Err("future snapshot cut was not invalid".into());
            }
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _=peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await;
    second.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
