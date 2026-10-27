//! Registration's result is durable before its outgoing answer is dropped.
use super::*;
use fleet_sync_protocol::{
    ClientFrame, DatabaseId, FrameId, LeaseRequest, Origin, PeerId, Registration,
    RegistrationAnswer,
};
use futures_util::SinkExt;

#[tokio::test]
async fn registration_replays_the_committed_answer_after_the_server_process_dies()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let mut peer = enroll_admitted_peer(
        &ConformanceClient::new(&first.base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let database = DatabaseId::new(Uuid::now_v7())?;
    for change in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("restart registration".to_owned())?,
            description: Description::try_from("ordinary Fleet creation".to_owned())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("restart board".to_owned())?,
            description: Description::try_from("registration parent".to_owned())?,
            board_kind: BoardKind::Area {},
        },
        creation_scenarios::project(database, board)?,
    ] {
        creation_scenarios::accepted(
            creation_scenarios::metadata(&mut peer.socket, &workspace, change).await?,
        )?;
    }
    let request = Registration {
        attempt: fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
        database,
        writer: PeerId::new(peer.peer)?,
        origin: Origin::Empty,
    };
    let frame = FrameId::new(Uuid::now_v7())?;
    let address = ObjectAddress::Connection(SERVICE.to_owned().try_into()?);
    control(
        &first.base,
        address.clone(),
        RuntimeControl::DropNextAnswer { frame },
    )
    .await?;
    peer.socket
        .send(reqwest_websocket::Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Lease(LeaseRequest::Register(request.clone())),
            })?
            .into(),
        ))
        .await?;
    let observed = control(
        &first.base,
        address,
        RuntimeControl::AwaitDroppedAnswer { frame },
    )
    .await?;
    assert!(
        matches!(observed.answer,collaboration_central_conformance::AnswerFaultObservation::Dropped {frame:actual} if actual==frame)
    );
    let path = root.path().join("registry.db");
    let (id, result) = journal_result(&path, "fleet-sync.register").await?;
    let expected: RegistrationAnswer = serde_json::from_slice(&result)?;
    creation_scenarios::registered(&expected, request.writer)?;
    first.crash().await?;
    drop(peer);

    let second = ServerProcess::start(root.path(), &jwks.url).await?;
    // Reconnect the original enrolled writer directly: its Welcome now contains
    // the grant whose outgoing registration answer was dropped before the crash.
    let mut socket = reconnect(&second.base).await?;
    let actual = creation_scenarios::operation_answer(
        &mut socket,
        OperationRequest::Lease(LeaseRequest::Register(request.clone())),
    )
    .await?;
    assert_eq!(
        serde_json::to_value(actual)?,
        json!({"kind":"lease","value":{"kind":"register","value":expected}})
    );
    let again = creation_scenarios::operation_answer(
        &mut socket,
        OperationRequest::Lease(LeaseRequest::Register(request)),
    )
    .await?;
    assert_eq!(
        serde_json::to_value(again)?,
        json!({"kind":"lease","value":{"kind":"register","value":expected}})
    );
    drop(socket);
    second.stop().await?;
    assert_eq!(
        journal_result(&path, "fleet-sync.register").await?,
        (id, result)
    );
    assert_eq!(
        read_count(
            &path,
            "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='fleet-sync.register'"
        )
        .await?,
        1
    );
    jwks.shutdown().await?;
    Ok(())
}

pub(super) async fn reconnect(base: &str) -> ConformanceResult<reqwest_websocket::WebSocket> {
    reconnect_peer(base, CREDENTIAL, 1).await
}

pub(super) async fn reconnect_peer(
    base: &str,
    credential: &str,
    max_schema_version: u64,
) -> ConformanceResult<reqwest_websocket::WebSocket> {
    Ok(
        reconnect_peer_with_welcome(base, credential, max_schema_version)
            .await?
            .0,
    )
}

pub(super) async fn reconnect_peer_with_welcome(
    base: &str,
    credential: &str,
    max_schema_version: u64,
) -> ConformanceResult<(
    reqwest_websocket::WebSocket,
    Vec<fleet_sync_protocol::DatabaseStatus>,
)> {
    use reqwest_websocket::RequestBuilderExt;
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(credential)
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion(credential)?,
        )
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let session = Uuid::now_v7();
    socket
        .send(reqwest_websocket::Message::Text(
            json!({"kind":"hello","value":{"sessionId":session,"schemaSupport":{"maxVersion":max_schema_version}}})
                .to_string(),
        ))
        .await?;
    let databases = read_reopened_welcome(&mut socket, session).await?;
    Ok((socket, databases))
}

async fn read_reopened_welcome<Messages>(
    messages: &mut Messages,
    session: Uuid,
) -> ConformanceResult<Vec<fleet_sync_protocol::DatabaseStatus>>
where
    Messages: futures_util::Stream<Item = Result<reqwest_websocket::Message, reqwest_websocket::Error>>
        + Unpin,
{
    // Notice delivery can run after attachment while Welcome awaits Registry/Copy.
    // Keep one total deadline, including all notices, and require the exact Hello UUID.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = messages
                .next()
                .await
                .ok_or("reopened registration socket closed")??;
            let frame: ServerFrame<BoardConsumerProtocol> = match message {
                reqwest_websocket::Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                reqwest_websocket::Message::Text(text) => serde_json::from_str(&text)?,
                _ => return Err("reopened registration did not return Welcome".into()),
            };
            match frame {
                ServerFrame::Notice { .. } => {}
                ServerFrame::Welcome {
                    session_id,
                    databases,
                } if session_id.as_uuid() == session => return Ok(databases),
                ServerFrame::Welcome { session_id, .. } => return Err(format!(
                    "reopened registration Welcome identity mismatch: expected {session}, got {}",
                    session_id.as_uuid()
                )
                .into()),
                _ => {
                    return Err(
                        "reopened registration received another frame before Welcome".into(),
                    );
                }
            }
        }
    })
    .await?
}

#[tokio::test]
async fn reopened_welcome_accepts_notice_before_its_exact_session() -> ConformanceResult<()> {
    // Attached sessions may receive a notice while Welcome gathers Registry/Copy state.
    let session = fleet_sync_protocol::SessionId::new(Uuid::now_v7())?;
    let notice = ServerFrame::<BoardConsumerProtocol>::Notice {
        position: fleet_sync_protocol::NoticePosition::new(1),
        body: Notice::LeaseChanged {
            database: DatabaseId::new(Uuid::now_v7())?,
        },
    };
    let expected = vec![fleet_sync_protocol::DatabaseStatus::Unavailable {
        database: DatabaseId::new(Uuid::now_v7())?,
        lease: None,
        reason: fleet_sync_protocol::CopyUnavailableReason::Uninitialized,
    }];
    let welcome = ServerFrame::<BoardConsumerProtocol>::Welcome {
        session_id: session,
        databases: expected.clone(),
    };
    let mut messages = futures_util::stream::iter([
        Ok(reqwest_websocket::Message::Binary(
            serde_json::to_vec(&notice)?.into(),
        )),
        Ok(reqwest_websocket::Message::Text(serde_json::to_string(
            &welcome,
        )?)),
    ]);

    let databases = read_reopened_welcome(&mut messages, session.as_uuid()).await?;

    assert_eq!(databases, expected);
    assert!(
        messages.next().await.is_none(),
        "Exact Welcome must be consumed"
    );
    Ok(())
}

#[tokio::test]
async fn reopened_welcome_notices_do_not_restart_the_total_deadline() -> ConformanceResult<()> {
    let session = Uuid::now_v7();
    let notice = serde_json::to_string(&ServerFrame::<BoardConsumerProtocol>::Notice {
        position: fleet_sync_protocol::NoticePosition::new(1),
        body: Notice::LeaseChanged {
            database: DatabaseId::new(Uuid::now_v7())?,
        },
    })?;
    let mut messages = Box::pin(futures_util::stream::unfold(notice, |notice| async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        Some((Ok(reqwest_websocket::Message::Text(notice.clone())), notice))
    }));

    let result = tokio::time::timeout(
        Duration::from_secs(6),
        read_reopened_welcome(&mut messages, session),
    )
    .await?;

    assert!(
        result.err().is_some_and(|error| error
            .downcast_ref::<tokio::time::error::Elapsed>()
            .is_some()),
        "Notices must not extend the five-second total Welcome bound"
    );
    Ok(())
}

#[tokio::test]
async fn reopened_welcome_rejects_a_changed_session_or_another_frame() -> ConformanceResult<()> {
    let session = Uuid::now_v7();
    for frame in [
        ServerFrame::<BoardConsumerProtocol>::Welcome {
            session_id: fleet_sync_protocol::SessionId::new(Uuid::now_v7())?,
            databases: Vec::new(),
        },
        ServerFrame::Closed {
            reason: fleet_sync_protocol::CloseReason::SessionEnded,
        },
    ] {
        let valid_welcome = ServerFrame::<BoardConsumerProtocol>::Welcome {
            session_id: fleet_sync_protocol::SessionId::new(session)?,
            databases: Vec::new(),
        };
        let mut messages = futures_util::stream::iter([
            Ok(reqwest_websocket::Message::Binary(
                serde_json::to_vec(&frame)?.into(),
            )),
            Ok(reqwest_websocket::Message::Binary(
                serde_json::to_vec(&valid_welcome)?.into(),
            )),
        ]);

        let result = read_reopened_welcome(&mut messages, session).await;

        assert!(
            result.is_err(),
            "A different session or frame must not satisfy Welcome"
        );
        assert!(
            messages.next().await.is_some(),
            "Unexpected frames must fail immediately rather than skip to a valid Welcome"
        );
    }
    Ok(())
}
