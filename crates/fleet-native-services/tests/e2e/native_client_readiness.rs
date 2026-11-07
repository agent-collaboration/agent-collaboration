use crate::consumer_fixture::*;
use crate::support::*;
use durable_steps::{CallFailure, NotSentReason, Uuid};
use fleet_protocol::*;
use fleet_session::*;
use fleet_session_client::SessionPump;
use fleet_websocket_transport::WebSocketTransport;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::time::Duration;
use tokio::sync::watch;

#[tokio::test]
async fn production_pump_refuses_unsent_operations_until_the_matching_welcome() -> TestResult {
    let root = tempfile::tempdir()?;
    let service = ServiceProcess::start(&root.path().join("service")).await?;
    let mut control = service
        .connect("control", "Bearer consumer-control")
        .await?;
    let peer = PeerId::new(Uuid::from_u128(0x019eef00000070008000000000000004))?;
    schema_request(
        &mut control,
        SchemaControlRequest::HoldReport {
            peer,
            attach_seq: AttachSequence::new(1)?,
        },
    )
    .await?;
    let client = SessionClient::<_, _, SampleProtocol>::load(
        WebSocketTransport::default(),
        ConsumerStore::open(&root.path().join("client.db")).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 5 },
        Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_secs(1),
            factor: 2,
        },
    )
    .await?;
    let session = client.state().session_id;
    let (requests, mut events, pump) = SessionPump::new(
        client,
        Endpoint::new(&format!("ws://127.0.0.1:{}/session", service.port))?,
        PeerCredential::new("Bearer peer-4")?,
        Duration::from_secs(10),
    );
    let (stop, stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run(stopped));
    assert!(
        matches!(schema_request(&mut control, SchemaControlRequest::WaitReport).await?,
        SchemaControlAnswer::Report(report) if report.session_id == session)
    );
    let before = requests
        .request(OperationRequest::Consumer(SampleRequest::Echo {
            text: "before Welcome".into(),
        }))
        .await;
    // Shut down both owners even when the pre-fix result exposes the defect.
    if !matches!(
        before,
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { .. }
        })
    ) {
        stop.send(true)?;
        pumping.await??;
        schema_request(&mut control, SchemaControlRequest::ReleaseReport).await?;
        service.stop(&mut control).await?;
        return Err(format!("pre-Welcome operation must be NotSent: {:?}", before.err()).into());
    }
    assert!(matches!(
        control_request(&mut control, ControlRequest::RequestCount).await?,
        ControlAnswer::RequestCount(0)
    ));
    schema_request(&mut control, SchemaControlRequest::ReleaseReport).await?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(15), events.recv()).await?,
        Some(ClientEvent::Welcome { .. })
    ));
    assert!(
        matches!(requests.request(OperationRequest::Consumer(SampleRequest::Echo {
        text: "after Welcome".into(),
    })).await?, OperationAnswer::Consumer(SampleAnswer::Echo { text, .. }) if text == "after Welcome")
    );
    assert!(matches!(
        control_request(&mut control, ControlRequest::RequestCount).await?,
        ControlAnswer::RequestCount(1)
    ));
    stop.send(true)?;
    pumping.await??;
    service.stop(&mut control).await?;
    Ok(())
}
