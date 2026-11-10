use super::*;
use futures_util::{SinkExt as _, StreamExt as _};
use router_clock::{ClockInstant, ManualClock, RuntimeClock};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn hold_native_request(
    listener: tokio::net::UnixListener,
    reached: oneshot::Sender<Value>,
    release: oneshot::Receiver<()>,
) -> TestResult {
    let (stream, _) = listener.accept().await?;
    let mut socket = tokio_tungstenite::accept_async(stream).await?;
    let initialize: Value =
        serde_json::from_str(socket.next().await.ok_or("initialize absent")??.to_text()?)?;
    socket
        .send(Message::Text(
            json!({"id":initialize.get("id"),"result":{}})
                .to_string()
                .into(),
        ))
        .await?;
    let _initialized = socket.next().await.ok_or("initialized absent")??;
    let request = serde_json::from_str(socket.next().await.ok_or("request absent")??.to_text()?)?;
    reached
        .send(request)
        .map_err(|_| "request observer ended")?;
    let _ = release.await;
    Ok(())
}

fn native_schemas() -> TestResult<Arc<codex_native_integration::NativePayloadSchemas>> {
    let mut definitions = serde_json::Map::new();
    for operation in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{operation}Params"), json!({"type":"object"}));
        definitions.insert(format!("{operation}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    Ok(Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)?,
    ))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the native request timeout contract."
)]
async fn a_summary_request_waits_twenty_nine_seconds_and_times_out_at_thirty_on_its_clock()
-> TestResult {
    // Arrange a real native carrier that receives the request but never answers it.
    let directory = tempfile::Builder::new()
        .prefix("summary-request")
        .tempdir_in("/tmp")?;
    let path = directory.path().join("native.sock");
    let listener = tokio::net::UnixListener::bind(&path)?;
    let (reached, received) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let server = tokio::spawn(hold_native_request(listener, reached, held));
    let gate = crate::NativeGenerationGate::default();
    gate.activate(
        serde_json::from_value(
            json!({"serviceEpoch":"0192a1b2-c3d4-7000-8000-000000000001","generation":1}),
        )?,
        path.clone(),
        Some(native_schemas()?),
    )?;
    let admission = gate.acquire()?;
    let mut connection = NativeProtocolConnection::connect(&path).await?;
    let clock = Arc::new(ManualClock::new("2001-01-01T00:00:00Z".parse()?));
    let request = tokio::spawn({
        let clock = clock.clone();
        async move {
            validated_call(
                clock.as_ref(),
                &admission,
                &mut connection,
                NativeOperation::InterruptTurn,
                json!({"threadId":"summary-thread","turnId":"summary-turn"}),
            )
            .await
        }
    });
    let observed = received.await?;
    assert_eq!(observed.get("method"), Some(&json!("turn/interrupt")));
    assert_eq!(
        observed.pointer("/params/turnId"),
        Some(&json!("summary-turn"))
    );
    let deadline = ClockInstant::ZERO + Duration::from_secs(30);
    assert!(
        clock.has_not_due_check_now(deadline),
        "the summary request must arm its injected bound"
    );

    // Act: the fixture still holds the actual response throughout both advances.
    clock.advance(Duration::from_secs(29))?;
    clock.not_due_check_now(deadline).await;
    assert!(!request.is_finished());
    clock.advance(Duration::from_secs(1))?;

    // Assert the original thirty-second bound before allowing the server to close.
    assert!(matches!(
        request.await?,
        Err(NativeConnectionError::OutcomeUnknown)
    ));
    assert_eq!(clock.monotonic_now(), deadline);
    release
        .send(())
        .map_err(|_| "native server stopped before release")?;
    server.await??;
    Ok(())
}
