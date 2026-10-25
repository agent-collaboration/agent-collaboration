//! Conflicts preserve the exact canonical records, head and original decision.
use crate::{
    ConformanceClient, ConformanceResult, composed_client::operation, enroll_admitted_peer,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardReadPage, BoardReadScope,
    FleetRequestEnvelope,
};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use message_board::{ChangeOutcome, Record, RequestDecision};
use reqwest_websocket::WebSocket;

pub async fn run_composed_fleet_conflict(client: &ConformanceClient) -> ConformanceResult<()> {
    let id = durable_steps::Uuid::now_v7();
    let mut peer = enroll_admitted_peer(
        client,
        &id.to_string(),
        &durable_steps::Uuid::now_v7().to_string(),
        &format!("fleet-conflict-{id}"),
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let proof=async {
        let offered=FleetRequest {request_id:message_board::RequestId::try_from(durable_steps::Uuid::now_v7().to_string())?,workspace:message_board::WorkspaceId::try_from(durable_steps::Uuid::now_v7().to_string())?,actor:message_board::RecordedActor::Human {human_id:message_board::HumanId::try_from("owner".to_owned())?},acting_for:None,protocol_version:message_board::ProtocolVersion::CURRENT,operation:FleetOperation::WorkspaceCreate {name:message_board::ResourceName::try_from(format!("conflict {id}"))?,description:message_board::Description::try_from("original exact content".to_owned())?}};
        let original=write(&mut peer.socket,&offered).await?;
        if !matches!(original,FleetRequestResult::Decided(RequestDecision::Accepted {change:ChangeOutcome::Changed,..})) {return Err("conflict fixture was not accepted".into());}
        let before=records(&mut peer.socket).await?;
        equal(write(&mut peer.socket,&offered).await?,original.clone())?;
        equal(records(&mut peer.socket).await?,before.clone())?;
        let mut changed=offered.clone();
        changed.operation=FleetOperation::WorkspaceUpdate {name:message_board::ResourceName::try_from("conflicting content".to_owned())?,description:message_board::Description::try_from("changed".to_owned())?};
        equal(write(&mut peer.socket,&changed).await?,FleetRequestResult::ConflictingRequest)?;
        equal(records(&mut peer.socket).await?,before.clone())?;
        equal(write(&mut peer.socket,&offered).await?,original)?;
        equal(records(&mut peer.socket).await?,before)?;
        println!("COMPOSED_FLEET_CONFLICT exact_original_decision_canonical_records_and_head_unchanged passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}
async fn write(
    socket: &mut WebSocket,
    offered: &FleetRequest,
) -> ConformanceResult<FleetRequestResult> {
    match operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: offered.request_id.clone(),
            operation: BoardPayload::encode(offered)?,
        })),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) if request_id == offered.request_id => Ok(decision.decode()?),
        _ => Err("conflict decision lost its exact request identity".into()),
    }
}
async fn records(socket: &mut WebSocket) -> ConformanceResult<(Vec<Record>, u64)> {
    match operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetSnapshot,
            cursor: None,
        }),
    )
    .await?
    {
        OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetSnapshot {
            records,
            next: None,
            snapshot_at,
            ..
        })) => Ok((
            records
                .into_iter()
                .map(|record| record.decode().map_err(Into::into))
                .collect::<ConformanceResult<Vec<_>>>()?,
            snapshot_at.get(),
        )),
        _ => Err("conflict canonical records did not complete".into()),
    }
}
fn equal<T: PartialEq + std::fmt::Debug>(actual: T, expected: T) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("conflict expected {expected:?}, got {actual:?}").into());
    }
    Ok(())
}
