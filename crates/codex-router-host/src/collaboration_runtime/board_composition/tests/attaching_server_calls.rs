//! SO53 ordering fixture: Welcome waits for real native server-call answers.
use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::{CallId, Uuid};
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DatabaseId, Epoch, FrameId, ReleaseAnswer, ReleaseRequest,
    ServerCall, ServerCallAnswer, ServerFrame, ServerRequest, TransferId, WithdrawAnswer,
    WithdrawRelease,
};
use fleet_sync_session::Endpoint;
use runtime_primitives::{OperationId, StepName};
use tokio::{net::TcpListener, sync::mpsc};

fn call(step: &str) -> Result<CallId, Box<dyn std::error::Error + Send + Sync>> {
    Ok(CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new(step)?,
    })
}

async fn receive(socket: &mut WebSocket) -> Result<ClientFrame<BoardConsumerProtocol>, io::Error> {
    match timeout(BOUND, socket.recv())
        .await
        .map_err(io::Error::other)?
    {
        Some(Ok(Message::Binary(bytes))) => {
            serde_json::from_slice(&bytes).map_err(io::Error::other)
        }
        _ => Err(io::Error::other("expected a typed client binary frame")),
    }
}

async fn send(socket: &mut WebSocket, frame: ServerFrame<BoardConsumerProtocol>) -> io::Result<()> {
    socket
        .send(Message::Binary(
            serde_json::to_vec(&frame).map_err(io::Error::other)?.into(),
        ))
        .await
        .map_err(io::Error::other)
}

async fn exchange_before_welcome(socket: &mut WebSocket, database: DatabaseId) -> io::Result<()> {
    let ClientFrame::Hello { session_id, .. } = receive(socket).await? else {
        return Err(io::Error::other("Hello must be the first client frame"));
    };
    let transfer = TransferId::new(Uuid::now_v7()).map_err(io::Error::other)?;
    let release_frame = FrameId::new(Uuid::now_v7()).map_err(io::Error::other)?;
    send(
        socket,
        ServerFrame::Request {
            frame: release_frame,
            body: ServerRequest::Crate(ServerCall::Release {
                call: call("attaching-release").map_err(io::Error::other)?,
                request: ReleaseRequest {
                    database,
                    epoch: Epoch::new(1).map_err(io::Error::other)?,
                    transfer,
                },
            }),
        },
    )
    .await?;
    match receive(socket)
        .await
        .map_err(|error| io::Error::other(format!("release answer before Welcome: {error}")))?
    {
        ClientFrame::Answer {
            frame,
            body: ClientAnswer::Crate(ServerCallAnswer::Release(ReleaseAnswer::Released { .. })),
        } if frame == release_frame => {}
        _ => {
            return Err(io::Error::other(
                "native release was not answered before Welcome",
            ));
        }
    }
    let withdrawal_frame = FrameId::new(Uuid::now_v7()).map_err(io::Error::other)?;
    send(
        socket,
        ServerFrame::Request {
            frame: withdrawal_frame,
            body: ServerRequest::Crate(ServerCall::WithdrawRelease {
                call: call("attaching-withdrawal").map_err(io::Error::other)?,
                request: WithdrawRelease {
                    database,
                    epoch: Epoch::new(1).map_err(io::Error::other)?,
                    transfer,
                },
            }),
        },
    )
    .await?;
    match receive(socket).await? {
        ClientFrame::Answer {
            frame,
            body: ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(WithdrawAnswer::Withdrawn)),
        } if frame == withdrawal_frame => {}
        _ => {
            return Err(io::Error::other(
                "native withdrawal was not answered before Welcome",
            ));
        }
    }
    // The fixture makes the dependency explicit: sending Welcome any earlier
    // would let a wrongly gated Host pass without answering while attaching.
    send(
        socket,
        ServerFrame::Welcome {
            session_id,
            databases: Vec::new(),
        },
    )
    .await
}

#[tokio::test]
async fn native_release_and_withdrawal_are_answered_before_welcome() -> TestResult {
    // Arrange a real registered native writer through the generic FleetService.
    // Only the subsequent peer's ordering is scripted; native release, persisted
    // authority, Reqwest transport and the Host event dispatcher remain real.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut fleet_service =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    let board = open_board_admission(&root, identity()?, Some(fleet_service.inputs()?)).await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .projects
        .clone();
    let ready = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .welcomed
        .subscribe();
    let (fleet, request) = arrange_project(&board, &root).await?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    timeout(BOUND, fleet_service.auth.entered.notified()).await?;
    assert_eq!(
        settle_fixture_directory(&fleet, &request).await?,
        Some(ProjectRegistrationState::Pending)
    );
    fleet_service.release()?;
    wait_welcome(ready).await?;
    collaboration_contracts::ProjectRegistration::start(projects.as_ref(), &request.project_id)
        .await?;
    let writer = projects.ensure(&request.project_id).await?;
    assert!(matches!(
        timeout(BOUND, writer.runtime.wait_registration()).await??,
        WriterRegistrationOutcome::Ready { .. }
    ));
    timeout(BOUND, projects.settle_operations()).await??;
    let accepted = admission.create_project(&request).await?.result;
    assert!(matches!(
        accepted,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    drop(writer);
    drop(fleet);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;
    drop(projects);
    fleet_service.stop().await?;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut inputs = fleet_service.inputs()?;
    inputs.endpoint = Endpoint::new(&format!("ws://{}/session", listener.local_addr()?))?;
    let database = DatabaseId::new(Uuid::parse_str(request.project_id.as_str())?)?;
    let (report, mut reports) = mpsc::channel(1);
    let router = axum::Router::new().route(
        "/session",
        axum::routing::get(move |upgrade: WebSocketUpgrade| {
            let report = report.clone();
            async move {
                upgrade.on_upgrade(move |mut socket| async move {
                    let result = exchange_before_welcome(&mut socket, database).await;
                    let _ = report.send(result).await;
                    while let Some(Ok(message)) = socket.recv().await {
                        if matches!(message, Message::Close(_)) {
                            break;
                        }
                    }
                })
            }
        }),
    );
    let (stop, mut stopped) = watch::channel(false);
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.changed().await;
            })
            .await
    });

    // Act through the reopened Host factory and its real session pump.
    let board = open_board_admission(&root, identity()?, Some(inputs)).await?;
    let ready = board
        .session
        .as_ref()
        .ok_or("resumed session missing")?
        .welcomed
        .subscribe();
    let admission = board.admission.clone();
    let running = board.start().ok_or("resumed session not started")?;
    timeout(BOUND + std::time::Duration::from_secs(5), reports.recv())
        .await?
        .ok_or("scripted peer report missing")??;
    wait_welcome(ready).await?;

    // Assert the local accepted record survived the release/withdrawal exchange.
    assert_eq!(admission.create_project(&request).await?.result, accepted);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("resumed admission still owned"))?
        .close()
        .await?;
    stop.send_replace(true);
    timeout(BOUND, server).await???;
    Ok(())
}
