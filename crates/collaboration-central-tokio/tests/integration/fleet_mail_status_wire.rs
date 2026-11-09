//! A recipient report must wake terminal status publication over the actual machine WebSockets.
use super::{
    composed_fleet_fixture::{A, B, SERVICE_A, SERVICE_B, TestResult},
    fleet_mail_dispatch::envelope,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardDeliveryTypes,
};
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_protocol::{
    ClientAnswer, ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryStatus, FrameId,
    OperationAnswer, OperationRequest, PushResult, RecipientOutcome, RecipientReport, ReportResult,
    ReportSeq, ServerCall, ServerCallAnswer, ServerFrame, ServerRequest, StatusCursor,
    StatusPublicationResult, SubmitResult,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

type Acceptance = <BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;

const CAP: Duration = Duration::from_secs(5);
pub(super) use crate::configuration;

pub(super) async fn send_client_frame(
    socket: &mut WebSocket,
    frame: ClientFrame<BoardConsumerProtocol>,
) -> TestResult {
    socket
        .send(Message::Binary(serde_json::to_vec(&frame)?.into()))
        .await?;
    Ok(())
}
pub(super) async fn read_server_frame(
    socket: &mut WebSocket,
) -> TestResult<ServerFrame<BoardConsumerProtocol>> {
    let message = tokio::time::timeout(CAP, socket.next())
        .await
        .map_err(|_| "status wire did not produce its causal next frame")?
        .ok_or("status wire closed")??;
    match message {
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        other => Err(format!("unexpected status transport frame: {other:?}").into()),
    }
}
fn new_call_id(step: &str) -> TestResult<durable_steps::CallId> {
    Ok(durable_steps::CallId {
        operation: durable_steps::OperationId::new(durable_steps::Uuid::now_v7())?,
        step: durable_steps::StepName::new(step)?,
    })
}
pub(super) async fn receive_and_ack_publication(
    socket: &mut WebSocket,
) -> TestResult<fleet_protocol::StatusPublication<Acceptance>> {
    loop {
        match read_server_frame(socket).await? {
            ServerFrame::Request {
                frame,
                body: ServerRequest::Crate(ServerCall::PublishStatus { publication, .. }),
            } => {
                send_client_frame(
                    socket,
                    ClientFrame::Answer {
                        frame,
                        body: ClientAnswer::Crate(ServerCallAnswer::Published(
                            StatusPublicationResult::Applied,
                        )),
                    },
                )
                .await?;
                return Ok(publication);
            }
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "expected status publication: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real wire must publish the terminal version once after the recipient's acknowledged report, with no alarm nudge."
)]
#[tokio::test]
async fn delivered_report_wakes_one_terminal_publication_to_the_submitters_real_socket()
-> TestResult {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        registry: configuration::registry_config()?,
        // No old peer-reply deadline may accidentally drive the newly owed status.
        holder_answer_timeout: Duration::from_secs(3_600),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof=async {
        let client=ConformanceClient::new(server.base_url())?;
        let mut sender=enroll_admitted_peer(&client,SERVICE_A,A,"terminal-status-a","019f0000-0000-7000-8000-000000008401").await?;
        let mut recipient=enroll_admitted_peer(&client,SERVICE_B,B,"terminal-status-b","019f0000-0000-7000-8000-000000008402").await?;
        let item=envelope(B,SERVICE_B)?;
        let item_id=item.id();
        let submit=FrameId::new(durable_steps::Uuid::now_v7())?;
        send_client_frame(&mut sender.socket,ClientFrame::Request {frame:submit,body:OperationRequest::Consumer(BoardConsumerRequest::MailSubmit {item:Box::new(item),origin_epoch:None})}).await?;
        assert!(matches!(read_server_frame(&mut sender.socket).await?,ServerFrame::Answer {frame,body:OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(board_sync_protocol::MailSubmitAnswer::Submitted(SubmitResult::Accepted {..})))} if frame==submit));
        let pushed=loop {
            match read_server_frame(&mut recipient.socket).await? {
                ServerFrame::Request {frame,body:ServerRequest::Crate(ServerCall::Push {item:pushed,..})}=>{assert_eq!(pushed.id(),item_id);break frame;},
                ServerFrame::Notice {..}=>{},
                other=>return Err(format!("expected Push: {}",serde_json::to_string(&other)?).into()),
            }
        };
        send_client_frame(&mut recipient.socket,ClientFrame::Answer {frame:pushed,body:ClientAnswer::Crate(ServerCallAnswer::Pushed(PushResult::Accepted {accepted_at:runtime_primitives::SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis())}))}).await?;
        let at_recipient=receive_and_ack_publication(&mut sender.socket).await?;
        assert_eq!(at_recipient.item,item_id);
        assert_eq!(at_recipient.status,DeliveryStatus::AtRecipient);
        let report=RecipientReport::<BoardDeliveryTypes> {item:item_id,report_seq:ReportSeq::new(1),outcome:RecipientOutcome::Delivered {acceptance:serde_json::from_str::<Acceptance>("\"queued\"")?}};
        for replay in [false,true] {
            let frame=FrameId::new(durable_steps::Uuid::now_v7())?;
            send_client_frame(&mut recipient.socket,ClientFrame::Request {frame,body:OperationRequest::Delivery(DeliveryRequest::Report {call:new_call_id("terminal_report")?,report:serde_json::from_slice(&serde_json::to_vec(&report)?)?})}).await?;
            assert!(matches!(read_server_frame(&mut recipient.socket).await?,ServerFrame::Answer {frame:actual,body:OperationAnswer::Delivery(DeliveryAnswer::Report(answer))} if actual==frame && if replay {matches!(answer,ReportResult::Replayed {report_seq} if report_seq==ReportSeq::new(1))} else {matches!(answer,ReportResult::Ack {report_seq} if report_seq==ReportSeq::new(1))}));
            if !replay {
                let terminal=receive_and_ack_publication(&mut sender.socket).await.map_err(|error|format!("recipient Report ACKed but Delivered was not published: {error}"))?;
                assert_eq!(terminal.item,item_id);
                assert_eq!(terminal.status,DeliveryStatus::Delivered {acceptance:serde_json::from_str::<Acceptance>("\"queued\"")?});
                assert!(terminal.version>at_recipient.version);
            }
        }
        let barrier=FrameId::new(durable_steps::Uuid::now_v7())?;
        send_client_frame(&mut sender.socket,ClientFrame::Request {frame:barrier,body:OperationRequest::Delivery(DeliveryRequest::StatusFetch {call:new_call_id("terminal_status_barrier")?,after:StatusCursor::new(0),limit:100})}).await?;
        loop {
            match read_server_frame(&mut sender.socket).await? {
                ServerFrame::Answer {frame,body:OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(page))} if frame==barrier=>{
                    assert_eq!(page.events.iter().filter(|event|event.item==item_id&&matches!(event.status,DeliveryStatus::Delivered {..})).count(),1);
                    break;
                },
                ServerFrame::Notice {..}=>{},
                other=>return Err(format!("duplicate publication or unexpected barrier frame: {}",serde_json::to_string(&other)?).into()),
            }
        }
        sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await?;
        recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}
