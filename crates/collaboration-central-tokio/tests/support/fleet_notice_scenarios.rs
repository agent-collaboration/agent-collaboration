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
use fleet_protocol::{
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

/// The same typed client sockets survive one directory write and then carry mail.
pub async fn one_write_then_mail(base: &str) -> ConformanceResult<()> {
    use board_sync_protocol::{BoardDeliveryTypes, MailSubmitAnswer};
    use fleet_protocol::{
        DeliveryEnvelope, DeliveryItem, ItemId, PeerId, ServerCall, ServerRequest, SubmitResult,
    };
    let client = ConformanceClient::new(base)?;
    let sender_service = Uuid::now_v7().to_string();
    let recipient_service = Uuid::now_v7().to_string();
    let mut sender = enroll_admitted_peer(
        &client,
        &sender_service,
        &Uuid::now_v7().to_string(),
        "paired sender",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let mut recipient = enroll_admitted_peer(
        &client,
        &recipient_service,
        &Uuid::now_v7().to_string(),
        "paired recipient",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        if !matches!(feed(&mut recipient.socket, 0).await?, FleetFeedRead::Page {records,head,..} if records.is_empty() && head.get()==0) {
            return Err("paired recipient did not start on an empty Fleet".into());
        }
        let request = FleetRequest {
            request_id: RequestId::generate(), workspace: WorkspaceId::generate(),
            actor: RecordedActor::Human {human_id: HumanId::try_from("paired owner".to_owned())?},
            acting_for: None, protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceCreate {name: ResourceName::try_from("paired write".to_owned())?,description: Description::try_from("one committed write before mail".to_owned())?},
        };
        if !matches!(decide(&mut sender.socket, &request).await?, FleetRequestResult::Decided(RequestDecision::Accepted {change: ChangeOutcome::Changed,..})) {
            return Err("paired workspace creation was not accepted".into());
        }
        hint(&mut recipient.socket, 1).await?;
        if !matches!(feed(&mut recipient.socket, 0).await?, FleetFeedRead::Page {records,through,head,..} if records.len()==2 && through.get()==2 && head.get()==2) {
            return Err("paired read did not retain exactly one write and its receipt".into());
        }
        let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(Uuid::now_v7())?,
            serde_json::from_value(serde_json::json!({"kind":"humanUser","installation":sender_service}))?,
            serde_json::from_value(serde_json::json!({"peer":PeerId::new(recipient.peer)?,"address":{"endpoint":{"serviceId":recipient_service,"endpointId":"codex-local"},"sessionId":"paired-recipient"}}))?,
            None,
            serde_json::from_value(serde_json::json!({"kind":"message","text":"mail after Fleet write","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
        )?);
        let expected = serde_json::to_value(&item)?;
        if !matches!(operation(&mut sender.socket, BoardConsumerRequest::MailSubmit {item:Box::new(item),origin_epoch:None}).await?, BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {..}))) {
            return Err("mail after directory mutation was refused".into());
        }
        match next(&mut recipient.socket).await? {
            ServerFrame::Request {body: ServerRequest::Crate(ServerCall::Push {item,..}),..}
                if serde_json::to_value(&item)? == expected => {},
            _ => return Err("exact original Push did not follow the Fleet mutation on the existing recipient socket".into()),
        }
        println!("SESSION_PAIR_CASE one_Fleet_write_then_original_mail_Push_on_same_typed_client_sockets passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _ = sender
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let _ = recipient
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
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
            ServerFrame::Request {
                body:
                    fleet_protocol::ServerRequest::Crate(fleet_protocol::ServerCall::PublishStatus {
                        ..
                    }),
                ..
            } => {}
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
async fn hint(socket: &mut WebSocket, position: u64) -> ConformanceResult<()> {
    match next(socket).await? {
        ServerFrame::Notice {
            body:
                Notice::Consumer(PositionedBoardNotice {
                    position: actual,
                    kind: BoardNotice::ProjectListChanged {},
                }),
            ..
        } if actual.value() == position => Ok(()),
        other => Err(format!(
            "expected ProjectListChanged machine notice {position}, got {}",
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
        hint(&mut reader.socket, 1).await?;
        hint(&mut other_reader.socket, 1).await?;
        if decide(&mut writer.socket, &request).await? != accepted {
            return Err("replaying the mutation changed its decision".into());
        }
        let expected_notice = BoardNotice::ProjectListChanged {};
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
        hint(&mut reader.socket, 2).await?;
        hint(&mut other_reader.socket, 2).await?;
        if !matches!(feed(&mut reader.socket, 2).await?, FleetFeedRead::Page {records,through,head,..}
            if records.len()==2 && through.get()==4 && head.get()==4) {
            return Err("project-list hint did not lead to the second exact committed prefix".into());
        }

        let expected = vec![
            PositionedBoardNotice {position: fleet_protocol::NoticePosition::new(1),kind: expected_notice},
            PositionedBoardNotice {position: fleet_protocol::NoticePosition::new(2),kind: BoardNotice::ProjectListChanged {}},
        ];
        for socket in [&mut reader.socket, &mut other_reader.socket] {
            if !matches!(operation(socket, BoardConsumerRequest::Read {scope: BoardReadScope::Notices, cursor: None}).await?,
                BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {notices,..})) if notices==expected) {
                return Err("installations did not retain their own two exact numbered hints".into());
            }
        }
        archive_hint(&mut writer.socket, &request).await?;
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

async fn archive_hint(socket: &mut WebSocket, template: &FleetRequest) -> ConformanceResult<()> {
    use fleet_protocol::{DatabaseId, Epoch, NoticePosition, PeerId, RequestId as LeaseRequestId};
    let board = message_board::BoardId::generate();
    let project = message_board::ProjectId::generate();
    let mut request = template.clone();
    for operation in [
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: "Notice archive board".to_owned().try_into()?,
            description: "Normal holder archive hint".to_owned().try_into()?,
            board_kind: message_board::BoardKind::Area {},
        },
        FleetOperation::ProjectDirectoryCreate {
            project: project.clone(),
            board: board.clone(),
            title: "Notice archive project".to_owned().try_into()?,
        },
    ] {
        request.request_id = RequestId::generate();
        request.operation = operation;
        if !matches!(
            decide(socket, &request).await?,
            FleetRequestResult::Decided(RequestDecision::Accepted {
                change: ChangeOutcome::Changed,
                ..
            })
        ) {
            return Err("archive notice setup was not committed".into());
        }
    }
    let holder = PeerId::try_from("019f0000-0000-7000-8000-000000004981".to_owned())?;
    let registered = crate::creation_scenarios::register(
        socket,
        DatabaseId::try_from(project.as_str().to_owned())?,
        holder,
        LeaseRequestId::new(Uuid::now_v7())?,
    )
    .await?;
    crate::creation_scenarios::registered(&registered, holder)?;
    let grant = PositionedBoardNotice {
        position: NoticePosition::new(1),
        kind: BoardNotice::LeaseGranted {
            project,
            epoch: Epoch::new(1)?,
        },
    };
    request.request_id = RequestId::generate();
    request.operation = FleetOperation::BoardArchive { board };
    let accepted = decide(socket, &request).await?;
    if !matches!(
        accepted,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ) {
        return Err("held board archive was not committed".into());
    }
    let expected = vec![
        grant.clone(),
        PositionedBoardNotice {
            position: NoticePosition::new(2),
            kind: BoardNotice::ProjectListChanged {},
        },
    ];
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {
                notices, ..
            })) = operation(
                socket,
                BoardConsumerRequest::Read {
                    scope: BoardReadScope::Notices,
                    cursor: None,
                },
            )
            .await?
            else {
                return Err("archive notice read returned another page".into());
            };
            if notices == expected {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if notices != vec![grant.clone()] {
                return Err(format!(
                    "archive did not preserve its grant and one project-list hint: {notices:?}"
                )
                .into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "board archive did not deliver its project-list hint")??;
    if decide(socket, &request).await? != accepted {
        return Err("archive replay changed its decision".into());
    }
    if !matches!(operation(socket, BoardConsumerRequest::Read {scope: BoardReadScope::Notices,cursor:None}).await?, BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {notices,..})) if notices == expected)
    {
        return Err("archive replay duplicated its project-list hint".into());
    }
    eprintln!("FLEET_NOTICE_CASE board_archive_delivers_project_list_changed_once passed");
    Ok(())
}
