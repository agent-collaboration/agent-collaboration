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
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("reopened registration socket closed")??;
    let welcome: ServerFrame<BoardConsumerProtocol> = match message {
        reqwest_websocket::Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
        reqwest_websocket::Message::Text(text) => serde_json::from_str(&text)?,
        _ => return Err("reopened registration did not return Welcome".into()),
    };
    match welcome {
        ServerFrame::Welcome {
            session_id,
            databases,
        } if session_id.as_uuid() == session => Ok((socket, databases)),
        _ => Err("reopened registration lost its exact Welcome session identity".into()),
    }
}
