//! Controls are observed through normal traffic at their owning composed object.

use crate::composed_client::copy_probe;
use crate::{
    AnswerFaultObservation, ComposedNumericScenario, ConformanceClient, ConformanceResult,
    DispatchFaultObservation, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    StorageFaultObservation, enroll_admitted_peer,
};
use board_sync_protocol::{
    BoardAnnotation, BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest,
    BoardPayload, BoardReadPage, BoardReadScope, FleetFeedRead, FleetRequestEnvelope,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, CopyStatusAnswer, CopyUnavailableReason, DatabaseId, Epoch,
    FrameId, OperationAnswer, OperationRequest, Piece, PieceContents, ReplicatedTransaction,
    ReplicationAnswer, ReplicationPosition, ReplicationRequest, RowChange, ServerFrame, SqlValue,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::{num::NonZeroU64, time::Duration};

pub async fn run_runtime_control_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let fixture = ComposedNumericScenario::prepare(client).await?;
    let mut peer = enroll_admitted_peer(
        client,
        &fixture.service.to_string(),
        &fixture.peer.to_string(),
        &fixture.credential,
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let copy = ObjectAddress::Copy(fleet_sync_protocol::DatabaseId::try_from(
        fixture.database.to_string(),
    )?);
    let session = ObjectAddress::Connection(message_board::ServiceId::try_from(
        fixture.service.to_string(),
    )?);
    let proof = async {
        refused_controls(client, session.clone()).await?;
        pin_fleet_clock(client, &mut peer.socket).await?;
        if !matches!(control(client, copy.clone(), RuntimeControl::Observe).await?.clock, crate::ClockObservation::Wall) {
            return Err("Fleet SetTime leaked to a different object's clock".into());
        }
        control(client, copy.clone(), RuntimeControl::CopyFull { after_writes: NonZeroU64::new(2).ok_or("positive write threshold")? }).await?;
        independent_copy_progress(client, &mut peer.socket, fixture.peer).await?;
        if !matches!(control(client, copy.clone(), RuntimeControl::Observe).await?.storage, StorageFaultObservation::Armed { .. }) {
            return Err("another physical copy consumed the selected copy's storage fault".into());
        }
        let third = row_piece(fixture.database, 2, 3)?;
        let failed = operation(&mut peer.socket, OperationRequest::Replication(ReplicationRequest::Piece(third))).await?;
        if !matches!(failed, OperationAnswer::Replication(ReplicationAnswer::CopyUnavailable { reason: CopyUnavailableReason::StorageFull })) {
            return Err("CopyFull did not hit the real composed copy write".into());
        }
        let observed = control(client, copy.clone(), RuntimeControl::Observe).await?;
        if !matches!(observed.storage, StorageFaultObservation::Triggered { after_writes, written } if after_writes.get() == 2 && written >= 2) {
            return Err("CopyFull did not observe the actual transaction's late writes".into());
        }
        require_status(&mut peer.socket, fixture.database, 2).await?;
        // The complete literal scalar snapshot, not only the control's reply,
        // must remain unchanged after the real store completes its rollback.
        SinkExt::close(&mut peer.socket).await?;
        fixture.verify(client).await?;
        peer = enroll_admitted_peer(client, &fixture.service.to_string(), &fixture.peer.to_string(), &fixture.credential, &durable_steps::Uuid::now_v7().to_string()).await?;

        let third = row_piece(fixture.database, 2, 3)?;
        let lost_frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        control(client, session.clone(), RuntimeControl::DropNextAnswer { frame: lost_frame }).await?;
        independent_read(client, fixture.database, 2, Some(lost_frame)).await?;
        send(&mut peer.socket, lost_frame, OperationRequest::Replication(ReplicationRequest::Piece(third.clone()))).await?;
        let observed = control(client, session.clone(), RuntimeControl::AwaitDroppedAnswer { frame: lost_frame }).await?;
        if !matches!(observed.answer, AnswerFaultObservation::Dropped { frame } if frame == lost_frame) {
            return Err("the selected physical answer was not dropped".into());
        }
        require_status(&mut peer.socket, fixture.database, 3).await?;
        let replay = operation(&mut peer.socket, OperationRequest::Replication(ReplicationRequest::Piece(third))).await?;
        if !matches!(replay, OperationAnswer::Replication(ReplicationAnswer::AlreadyApplied { through }) if through == position(3)?) {
            return Err("the lost answer did not retain its real copy receipt".into());
        }

        control(client, session.clone(), RuntimeControl::HoldNextDispatch { destination: copy.clone() }).await?;
        independent_read(client, fixture.database, 3, None).await?;
        let held_frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        let fourth = row_piece(fixture.database, 3, 4)?;
        send(&mut peer.socket, held_frame, OperationRequest::Replication(ReplicationRequest::Piece(fourth.clone()))).await?;
        let observed = control(client, session.clone(), RuntimeControl::AwaitHeldDispatch).await?;
        let DispatchFaultObservation::Held { destination, request } = observed.dispatch else { return Err("the selected dispatch never reached its hold".into()); };
        if destination != copy { return Err("dispatch hold selected another physical copy".into()); }
        let envelope: serde_json::Value = serde_json::from_slice(&request)?;
        let held: Piece<BoardAnnotation> = serde_json::from_value(envelope.pointer("/request/request/value").cloned().ok_or("held request omitted its exact piece")?)?;
        if held.database != fourth.database || held.digest != fourth.digest {
            return Err("held dispatch belongs to another write".into());
        }
        let mut observer = enroll_admitted_peer(client, &durable_steps::Uuid::now_v7().to_string(), &durable_steps::Uuid::now_v7().to_string(), "control-independent-reader", &durable_steps::Uuid::now_v7().to_string()).await?;
        let read = require_status(&mut observer.socket, fixture.database, 3).await;
        let closed = observer.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
        // Release the exact hold even when the independent read failed.
        let released = control(client, session.clone(), RuntimeControl::ResumeDispatch).await;
        read?; closed?; released?;
        let applied = receive(&mut peer.socket, held_frame).await?;
        if !matches!(applied, OperationAnswer::Replication(ReplicationAnswer::Applied { through }) if through == position(4)?) {
            return Err("resuming the real dispatch did not apply its exact piece".into());
        }
        require_status(&mut peer.socket, fixture.database, 4).await?;
        println!("RUNTIME_CONTROLS_CASE real_object_clock_late_storage_rollback_exact_answer_drop_and_owned_dispatch_hold passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    // Do not leave a dispatcher blocked on an early assertion failure.
    let _ = control(client, session, RuntimeControl::ResumeDispatch).await;
    let _ = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}

async fn refused_controls(
    client: &ConformanceClient,
    session: ObjectAddress,
) -> ConformanceResult<()> {
    // Rejection probes own their HTTP connections, so early refusal cannot
    // contaminate the positive control journey's persistent client pool.
    let rejection_client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let request = RuntimeControlRequest {
        address: session.clone(),
        command: RuntimeControl::Observe,
    };
    let response = rejection_client
        .post(format!("{}/admin/v1/conformance/runtime", client.base_url))
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("control auth negative request failed: {error}"))?;
    let status = response.status();
    let _body = response.bytes().await?;
    if status != reqwest::StatusCode::UNAUTHORIZED {
        return Err("runtime control admitted a request without owner Access".into());
    }
    let invalid = RuntimeControlRequest {
        address: session,
        command: RuntimeControl::CopyFull {
            after_writes: NonZeroU64::new(2).ok_or("positive threshold")?,
        },
    };
    let response = client
        .with_owner_access(
            rejection_client.post(format!("{}/admin/v1/conformance/runtime", client.base_url)),
        )?
        .json(&invalid)
        .send()
        .await
        .map_err(|error| format!("control wrong-object negative request failed: {error}"))?;
    let status = response.status();
    let _body = response.bytes().await?;
    if status != reqwest::StatusCode::CONFLICT {
        return Err("a storage fault targeted a connection instead of a copy".into());
    }
    Ok(())
}

async fn independent_copy_progress(
    client: &ConformanceClient,
    socket: &mut WebSocket,
    holder: fleet_sync_protocol::PeerId,
) -> ConformanceResult<()> {
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let created = copy_probe(client, serde_json::json!({"kind":"create","value":fleet_sync_protocol::CreateCopy {
        database, epoch: Epoch::new(1)?, holder: Some(holder), origin: fleet_sync_protocol::CopyOrigin::Empty,
    }})).await?;
    if created
        != serde_json::json!({"kind":"created","value":fleet_sync_protocol::CreateCopyAnswer::CopyCreated})
    {
        return Err("independent copy was not created".into());
    }
    let piece = Piece::<BoardAnnotation>::new(PieceContents {
        database,
        epoch: Epoch::new(1)?,
        continues_from: position(0)?,
        through: position(1)?,
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitSequence::new(1),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE independent_records(id TEXT NOT NULL PRIMARY KEY)".into(),
            ],
        }],
        part: None,
        annotation: None,
    })?;
    if !matches!(operation(socket, OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?, OperationAnswer::Replication(ReplicationAnswer::Applied { through }) if through == position(1)?)
    {
        return Err("selected CopyFull blocked a different physical copy".into());
    }
    require_status(socket, database, 1).await
}

async fn independent_read(
    client: &ConformanceClient,
    database: DatabaseId,
    sequence: u64,
    frame: Option<FrameId>,
) -> ConformanceResult<()> {
    let service = durable_steps::Uuid::now_v7();
    let mut observer = enroll_admitted_peer(
        client,
        &service.to_string(),
        &durable_steps::Uuid::now_v7().to_string(),
        &format!("control-other-session-{service}"),
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        match frame {
            Some(frame) => {
                send(&mut observer.socket, frame, OperationRequest::Replication(ReplicationRequest::CopyStatus { database })).await?;
                if !matches!(receive(&mut observer.socket, frame).await?, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked == position(sequence)?) { return Err("answer drop reached a different session with the same frame identity".into()); }
                Ok(())
            }
            None => require_status(&mut observer.socket, database, sequence).await,
        }
    }.await;
    let closed = observer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn control(
    client: &ConformanceClient,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(client
        .with_owner_access(
            client
                .client
                .post(format!("{}/admin/v1/conformance/runtime", client.base_url)),
        )?
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn pin_fleet_clock(
    client: &ConformanceClient,
    socket: &mut WebSocket,
) -> ConformanceResult<()> {
    let now = RegistryTime(crate::fixture_time()?.0 + chrono::TimeDelta::hours(1));
    // Both runtimes' clock capability has millisecond precision.
    let now = RegistryTime(
        chrono::DateTime::from_timestamp_millis(now.0.timestamp_millis()).ok_or("clock fixture")?,
    );
    control(
        client,
        ObjectAddress::Fleet,
        RuntimeControl::SetTime { now },
    )
    .await?;
    let workspace =
        message_board::WorkspaceId::try_from(durable_steps::Uuid::now_v7().to_string())?;
    let request = FleetRequest {
        request_id: message_board::RequestId::try_from(durable_steps::Uuid::now_v7().to_string())?,
        workspace: workspace.clone(),
        actor: message_board::RecordedActor::Human {
            human_id: message_board::HumanId::try_from("clock owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: message_board::ResourceName::try_from(format!("clock {workspace:?}"))?,
            description: message_board::Description::try_from(
                "actual owning Fleet clock".to_owned(),
            )?,
        },
    };
    let answer = operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request.request_id.clone(),
            operation: BoardPayload::encode(&request)?,
        })),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Fleet { decision, .. }) = answer else {
        return Err("clock case did not commit its actual Fleet write".into());
    };
    if !matches!(
        decision.decode::<FleetRequestResult>()?,
        FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ) {
        return Err("clock fixture metadata was not accepted".into());
    }
    let answer = operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(board_sync_protocol::BoardReadCursor::Fleet(
                message_board::FleetPosition::ZERO,
            )),
        }),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetFeed(
        FleetFeedRead::Page { records, .. },
    ))) = answer
    else {
        return Err("clock case did not read its real Fleet feed".into());
    };
    let records = records
        .into_iter()
        .map(|record| record.decode::<message_board::Record>())
        .collect::<Result<Vec<_>, _>>()?;
    if !records.iter().any(|record| matches!(record, message_board::Record::Workspace(value) if value.workspace_id == workspace && value.envelope.time == message_board::RecordTime::Router(now.0))) {
        return Err("SetTime did not reach the normal owning Fleet write".into());
    }
    Ok(())
}
fn position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}
fn row_piece(
    database: DatabaseId,
    from: u64,
    through: u64,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    let key = durable_steps::Uuid::now_v7().to_string();
    Ok(Piece::new(PieceContents {
        database,
        epoch: Epoch::new(1)?,
        continues_from: position(from)?,
        through: position(through)?,
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(through),
            rows: vec![RowChange::upsert(
                "scalar_records",
                vec![SqlValue::Text(key.clone())],
                vec![
                    SqlValue::Text(key),
                    SqlValue::Integer(55),
                    SqlValue::Integer(66),
                ],
            )?],
        }],
        part: None,
        annotation: None,
    })?)
}
async fn require_status(
    socket: &mut WebSocket,
    database: DatabaseId,
    sequence: u64,
) -> ConformanceResult<()> {
    if !matches!(operation(socket, OperationRequest::Replication(ReplicationRequest::CopyStatus { database })).await?, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked == position(sequence)?)
    {
        return Err("control changed the wrong copy or its acknowledged prefix".into());
    }
    Ok(())
}
async fn operation(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    send(socket, frame, body).await?;
    receive(socket, frame).await
}
async fn send(
    socket: &mut WebSocket,
    frame: FrameId,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<()> {
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    Ok(())
}
async fn receive(
    socket: &mut WebSocket,
    frame: FrameId,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let next = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await?
        .ok_or("control scenario session ended")??;
    let bytes = match next {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => return Err(format!("control scenario received {other:?}").into()),
    };
    match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body,
        } if actual == frame => Ok(body),
        _ => Err("control scenario lost its exact requesting frame".into()),
    }
}
