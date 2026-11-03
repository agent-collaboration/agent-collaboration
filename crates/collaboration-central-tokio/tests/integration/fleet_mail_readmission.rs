//! Actual HTTP enrollment and socket delivery resume the same retained custody.
use super::{
    composed_fleet_fixture::*,
    fleet_mail_dispatch::{CAP, envelope},
    fleet_mail_inbox_fixture::RecipientInbox,
    fleet_mail_status_wire::{
        configuration, read_server_frame, receive_and_ack_publication, send_client_frame,
    },
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardDeliveryTypes, MailSubmitAnswer,
};
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, RuntimeControl, RuntimeControlRequest,
    enroll_admitted_peer, fixture_policy, owner_assertion,
};
use collaboration_central_core::{
    ObjectAddress, RegistryTime, RevocationControl, RevocationControlInput, RevocationObservation,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DeliveryEnvelope, DeliveryStatus, FrameId, OperationAnswer,
    OperationRequest, PushResult, ServerCall, ServerCallAnswer, ServerFrame, ServerRequest,
    SubmitResult,
};
use serde_json::{Value, json};
use std::time::Duration;

const CREDENTIAL: &str = "fleet-readmission-recipient";

async fn owner_post(
    client: &reqwest::Client,
    base: &str,
    route: &str,
    body: &impl serde::Serialize,
) -> TestResult<Value> {
    Ok(client
        .post(format!("{base}{route}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn advance(client: &reqwest::Client, base: &str, now: &mut i64) -> TestResult {
    *now += 10_000;
    owner_post(
        client,
        base,
        "/admin/v1/conformance/runtime",
        &RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp_millis(*now)
                        .ok_or("fixture clock out of range")?,
                ),
            },
        },
    )
    .await?;
    Ok(())
}
fn enrollment() -> TestResult<Value> {
    let hash = ring::digest::digest(&ring::digest::SHA256, CREDENTIAL.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(json!({"kind":"enrollMachine","value":{
        "installation":SERVICE_B,"peer":B,"label":CREDENTIAL,
        "access_client_id":collaboration_central_conformance::access_client_id(CREDENTIAL)?,
        "credential_hash":hash,"expires_at":"2033-05-18T03:33:20Z"
    }}))
}
async fn wait_revoked(client: &reqwest::Client, base: &str, now: &mut i64) -> TestResult {
    tokio::time::timeout(CAP, async {
        loop {
            let observed: RevocationObservation = serde_json::from_value(
                owner_post(
                    client,
                    base,
                    "/admin/v1/conformance/revocation",
                    &RevocationControlInput {
                        service: SERVICE_B.to_owned().try_into()?,
                        control: RevocationControl::Observe,
                    },
                )
                .await?,
            )?;
            if !observed.enrollment.is_some_and(|row| row.revoked) {
                return Err("revoke did not persist its gate".into());
            }
            if matches!(
                observed.operation.map(|operation| operation.state),
                Some(durable_steps::OperationState::Completed { .. })
            ) {
                return Ok(());
            }
            advance(client, base, now).await?;
            tokio::task::yield_now().await;
        }
    })
    .await?
}
async fn readmit(client: &reqwest::Client, base: &str, now: &mut i64) -> TestResult {
    tokio::time::timeout(CAP, async {
        loop {
            let answer =
                owner_post(client, base, "/admin/v1/machines/enroll", &enrollment()?).await?;
            match answer.get("kind").and_then(Value::as_str) {
                Some("machineEnrolled") if answer.pointer("/value/peer") == Some(&json!(B)) => {
                    return Ok(());
                }
                Some("revocationInProgress" | "enrollmentInProgress") => {}
                _ => return Err(format!("same-peer readmission failed: {answer}").into()),
            }
            advance(client, base, now).await?;
            tokio::task::yield_now().await;
        }
    })
    .await?
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real HTTP revoke/readmit must preserve immutable queued content and one recipient custody commit; replay must retain the first acceptance."
)]
#[tokio::test]
async fn same_peer_readmission_pushes_and_receipts_the_pre_revoke_item_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_with_controls(
        TokioSettings {
            root: root.path().join("central"),
            access_policy: fixture_policy(),
            jwks_url: jwks.url.clone(),
            registry: configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp_millis(NOW)
                        .ok_or("fixture clock out of range")?,
                ),
            },
        }],
    )
    .await?;
    let http = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(CAP)
        .build()?;
    let inbox = RecipientInbox::open(&root.path().join("recipient"), Io::new(NOW)).await?;
    let proof = Box::pin(async {
        let mut now = NOW;
        let client = ConformanceClient::new(server.base_url())?;
        let mut sender = enroll_admitted_peer(&client,SERVICE_A,A,"fleet-readmission-sender","019f0000-0000-7000-8000-000000008411").await?;
        let enrolled = owner_post(&http,server.base_url(),"/admin/v1/machines/enroll",&enrollment()?).await?;
        assert_eq!(enrolled.pointer("/value/peer"),Some(&json!(B)));
        // B has no socket: the actual sender receives Fleet custody before revoke.
        let item = envelope(B,SERVICE_B)?;
        let item_id = item.id();
        let digest = item.digest().clone();
        let frozen = serde_json::to_vec(&item)?;
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        send_client_frame(&mut sender.socket,ClientFrame::Request {frame,body:OperationRequest::Consumer(BoardConsumerRequest::MailSubmit {item:Box::new(item),origin_epoch:None})}).await?;
        assert!(matches!(read_server_frame(&mut sender.socket).await?, ServerFrame::Answer {frame:answer,body:OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {status:DeliveryStatus::Sent,..})))} if answer==frame));
        let revoked = owner_post(&http,server.base_url(),"/admin/v1/machines/revoke",&json!({"kind":"revokeMachine","value":{"installation":SERVICE_B}})).await?;
        assert_eq!(revoked,json!({"kind":"machineRevoked"}));
        wait_revoked(&http,server.base_url(),&mut now).await?;
        readmit(&http,server.base_url(),&mut now).await?;
        let mut recipient = enroll_admitted_peer(&client,SERVICE_B,B,CREDENTIAL,"019f0000-0000-7000-8000-000000008412").await?;
        advance(&http,server.base_url(),&mut now).await?;
        let pushed = loop {
            match read_server_frame(&mut recipient.socket).await? {
                ServerFrame::Request {frame,body:ServerRequest::Crate(ServerCall::Push {item,..})}=>break (frame,item),
                ServerFrame::Notice {..}=>{},
                other=>return Err(format!("readmitted recipient expected retained Push: {}",serde_json::to_string(&other)?).into()),
            }
        };
        assert_eq!(pushed.1.id(),item_id);
        assert_eq!(pushed.1.digest(),&digest);
        assert_eq!(serde_json::to_vec(&pushed.1)?,frozen);
        let receipt = inbox.accept(pushed.1,collaboration_central_core::central_mailbox_limits()?).await?;
        assert!(matches!(receipt,PushResult::Accepted {..}));
        send_client_frame(&mut recipient.socket,ClientFrame::Answer {frame:pushed.0,body:ClientAnswer::Crate(ServerCallAnswer::Pushed(receipt))}).await?;
        let publication = receive_and_ack_publication(&mut sender.socket).await?;
        assert_eq!(publication.item,item_id);
        assert_eq!(publication.status,DeliveryStatus::AtRecipient);
        assert_eq!(inbox.count().await?,1);
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        let replay:DeliveryEnvelope<BoardDeliveryTypes> = serde_json::from_slice(&frozen)?;
        send_client_frame(&mut sender.socket,ClientFrame::Request {frame,body:OperationRequest::Consumer(BoardConsumerRequest::MailSubmit {item:Box::new(replay),origin_epoch:None})}).await?;
        loop {
            match read_server_frame(&mut sender.socket).await? {
                ServerFrame::Answer {frame:answer,body:OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Replayed {status:DeliveryStatus::AtRecipient,..})))} if answer==frame=>break,
                ServerFrame::Notice {..}=>{},
                other=>return Err(format!("accepted custody replay changed its result: {}",serde_json::to_string(&other)?).into()),
            }
        }
        assert_eq!(inbox.count().await?,1);
        sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await?;
        recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    let receiver = inbox.close().await;
    stopped?;
    keys?;
    receiver?;
    proof
}
