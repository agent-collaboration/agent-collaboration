//! Two ordinary sessions exercise the Fleet producer, alarm, and real notice receiver.
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardNotice, BoardPayload,
    BoardReadCursor, BoardReadPage, BoardReadScope, FleetFeedRead, FleetRequestEnvelope,
    NoticeRead, PositionedBoardNotice,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{
    ClientFrame, FrameId, Notice, OperationAnswer, OperationRequest, ServerFrame,
};
use futures_util::{SinkExt, StreamExt};
use message_board::{
    ChangeOutcome, Description, FleetPosition, HumanId, ProtocolVersion, RecordedActor,
    RequestDecision, RequestId, ResourceName, WorkspaceId,
};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

async fn next(socket: &mut WebSocket) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    let message = tokio::time::timeout(Duration::from_secs(8), socket.next())
        .await
        .map_err(|_| "Fleet mutation committed but its alarm did not deliver the reader's notice")?
        .ok_or("Fleet notice closed its session")??;
    let bytes = match message {
        Message::Text(text) => text.into_bytes(),
        Message::Binary(bytes) => bytes.to_vec(),
        other => return Err(format!("expected a notice or answer, got {other:?}").into()),
    };
    Ok(serde_json::from_slice(&bytes)?)
}
async fn operation(
    socket: &mut WebSocket,
    body: BoardConsumerRequest,
) -> ConformanceResult<BoardConsumerAnswer> {
    let frame = FrameId::new(Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Consumer(body),
            })?
            .into(),
        ))
        .await?;
    loop {
        match next(socket).await? {
            ServerFrame::Answer {
                frame: actual,
                body: OperationAnswer::Consumer(body),
            } if actual == frame => return Ok(body),
            ServerFrame::Notice { .. } => {}
            _ => {
                return Err(
                    "Fleet notice scenario lost its typed answer or frame correlation".into(),
                );
            }
        }
    }
}
async fn feed(socket: &mut WebSocket, after: u64) -> ConformanceResult<FleetFeedRead> {
    match operation(
        socket,
        BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(BoardReadCursor::Fleet(FleetPosition::try_from(after)?)),
        },
    )
    .await?
    {
        BoardConsumerAnswer::Read(BoardReadPage::FleetFeed(page)) => Ok(page),
        _ => Err("reader did not receive its Fleet page".into()),
    }
}
async fn decide(
    socket: &mut WebSocket,
    request: &FleetRequest,
) -> ConformanceResult<FleetRequestResult> {
    match operation(
        socket,
        BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request.request_id.clone(),
            operation: BoardPayload::encode(request)?,
        }),
    )
    .await?
    {
        BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        } if request_id == request.request_id => Ok(decision.decode()?),
        _ => Err("mutation did not receive its Fleet decision".into()),
    }
}
async fn hint(socket: &mut WebSocket, position: u64, through: u64) -> ConformanceResult<()> {
    match next(socket).await? {
        ServerFrame::Notice {
            body:
                Notice::Consumer(PositionedBoardNotice {
                    position: actual,
                    kind: BoardNotice::FleetFeedAdvanced { through: head },
                }),
            ..
        } if actual.value() == position && head.get() == through => Ok(()),
        other => Err(format!(
            "expected machine notice {position} through {through}, got {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}
pub async fn feed_advancement_reaches_reader_once(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut reader = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440497",
        "019f0000-0000-7000-8000-000000004971",
        "Fleet notice reader",
        "019f0000-0000-7000-8000-000000004972",
    )
    .await?;
    let mut writer = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440498",
        "019f0000-0000-7000-8000-000000004981",
        "Fleet notice writer",
        "019f0000-0000-7000-8000-000000004982",
    )
    .await?;
    let mut other_reader = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440499",
        "019f0000-0000-7000-8000-000000004991",
        "Second Fleet reader",
        "019f0000-0000-7000-8000-000000004992",
    )
    .await?;
    let proof = async {
        if !matches!(feed(&mut reader.socket, 0).await?, FleetFeedRead::Page {records,through,head,..}
            if records.is_empty() && through.get()==0 && head.get()==0) {
            return Err("reader registration did not start at the empty committed prefix".into());
        }
        if !matches!(feed(&mut other_reader.socket, 0).await?, FleetFeedRead::Page {records,head,..}
            if records.is_empty() && head.get()==0) {
            return Err("second installation did not register its own applied cursor".into());
        }
        let request = FleetRequest {
            request_id: RequestId::try_from(Uuid::now_v7().to_string())?,
            workspace: WorkspaceId::try_from(Uuid::now_v7().to_string())?,
            actor: RecordedActor::Human {human_id: HumanId::try_from("Fleet notice owner".to_owned())?},
            acting_for: None, protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceCreate {name: ResourceName::try_from("Notice source".to_owned())?, description: Description::try_from("Committed by another admitted machine".to_owned())?},
        };
        let accepted = decide(&mut writer.socket, &request).await?;
        if !matches!(&accepted, FleetRequestResult::Decided(RequestDecision::Accepted {change: ChangeOutcome::Changed,..})) {
            return Err(format!("notice source was not committed: {accepted:?}").into());
        }
        hint(&mut reader.socket, 1, 2).await?;
        hint(&mut other_reader.socket, 1, 2).await?;
        if decide(&mut writer.socket, &request).await? != accepted {
            return Err("replaying the mutation changed its decision".into());
        }
        let expected_notice = BoardNotice::FleetFeedAdvanced {through: FleetPosition::try_from(2)?};
        match operation(&mut reader.socket, BoardConsumerRequest::Read {scope: BoardReadScope::Notices, cursor: None}).await? {
            BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {notices,..}))
                if matches!(notices.as_slice(), [notice] if notice.position.value()==1 && notice.kind == expected_notice) => {},
            _ => return Err("mutation replay or delivery retry duplicated the durable machine notice".into()),
        }
        if !matches!(feed(&mut reader.socket, 0).await?, FleetFeedRead::Page {records,through,head,..}
            if records.len()==2 && through.get()==2 && head.get()==2) {
            return Err("notified reader could not obtain the exact committed prefix".into());
        }
        feed(&mut reader.socket, 2).await?;
        let mut update = request.clone();
        update.request_id = RequestId::try_from(Uuid::now_v7().to_string())?;
        update.operation = FleetOperation::WorkspaceUpdate {name: ResourceName::try_from("Notice source updated".to_owned())?, description: Description::try_from("A second committed prefix".to_owned())?};
        if !matches!(decide(&mut writer.socket, &update).await?, FleetRequestResult::Decided(RequestDecision::Accepted {change: ChangeOutcome::Changed,..})) {
            return Err("second notice source was not committed".into());
        }
        hint(&mut reader.socket, 2, 4).await?;
        hint(&mut other_reader.socket, 2, 4).await?;
        let expected = vec![
            PositionedBoardNotice {position: fleet_sync_protocol::NoticePosition::new(1),kind: expected_notice},
            PositionedBoardNotice {position: fleet_sync_protocol::NoticePosition::new(2),kind: BoardNotice::FleetFeedAdvanced {through: FleetPosition::try_from(4)?}},
        ];
        for socket in [&mut reader.socket, &mut other_reader.socket] {
            if !matches!(operation(socket, BoardConsumerRequest::Read {scope: BoardReadScope::Notices, cursor: None}).await?,
                BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {notices,..})) if notices==expected) {
                return Err("installations did not retain their own two exact numbered hints".into());
            }
        }
        eprintln!("FLEET_NOTICE_CASE committed_feed_advancement_delivers_once passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let reader_closed = reader
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let writer_closed = writer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let other_closed = other_reader
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    reader_closed?;
    writer_closed?;
    other_closed?;
    Ok(())
}
