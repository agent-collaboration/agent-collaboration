//! A recurring wake creates a fresh push identity for each real native delivery.
use agent_automation::{ExpiryRule, TimingRule, WakeupId};
use automation_storage::{AutomationStore, WakeCreate};
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_protocol::{
    CodexGeneration, EndpointDescription, EndpointId, EndpointRef, OperationId, PushHeaderFacts,
    PushId, PushKind, PushOrigin, RouterLink, RouterOriginRef, SavedMessage, SessionId, SessionRef,
    UuidIdentity,
};
use collaboration_runtime::CodexAppServerDeliveryRoute;
use collaboration_service::ApplicationFixture;
use router_clock::{ManualClock, RuntimeClock};

use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;

use collaboration_runtime::SessionDeliveryRouter;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use sqlx::Connection as _;
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

// U66 / MS-10: real worker firings are separated by at least five logical minutes.
const REPEAT_INTERVAL_SECONDS: u32 = 300;
const MINIMUM_REPEAT_GAP_MILLIS: i64 = 300_000;
const WAKE_BODY: &str = "Review the recurring report";
const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";
const SESSION_ID: &str = "fixture-recurring-target";

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct NativeQueueObservation {
    client_user_message_id: String,
    prompt_text: String,
    observed_at_ms: i64,
}

/// Waits until the wake worker is asleep with no task running and its one timer armed
/// for `due_ms`: nothing else is pending, so only that timer can fire the next delivery.
/// The nextest profile supplies the outer hang bound.
async fn asleep_with_timer_armed_for(
    observation: &collaboration_runtime::WorkerObservation,
    due_ms: i64,
) {
    (async {
        loop {
            let snapshot = observation.snapshot();
            if snapshot.asleep
                && snapshot.tasks_running == 0
                && snapshot
                    .armed_deadline
                    .map(runtime_primitives::SyncTime::unix_millis)
                    == Some(due_ms)
            {
                return;
            }
            observation.asleep_at_least(snapshot.sleeps + 1).await;
        }
    })
    .await;
}

#[tokio::test]
async fn recurring_wake_stores_and_dispatches_two_distinct_pushes() {
    let initial_time: chrono::DateTime<chrono::Utc> = "2026-11-04T00:00:00Z".parse().unwrap();
    let clock = Arc::new(ManualClock::new(initial_time));
    let root = tempfile::tempdir_in("/tmp").expect("fixture temp directory");
    let database = root.path().join("automation.sqlite");
    let store = Arc::new(tokio::sync::Mutex::new(
        AutomationStore::open(&database)
            .await
            .expect("open automation store"),
    ));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
            .await
            .expect("open interaction owner"),
    );
    let socket_path = root.path().join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind native socket");
    let service_identity: UuidIdentity = SERVICE_ID
        .to_owned()
        .try_into()
        .expect("valid service identity");
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch": SERVICE_ID,
        "generation": 1
    }))
    .expect("valid Codex generation");
    let target = SessionRef {
        endpoint: EndpointRef {
            service_id: service_identity.clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("valid endpoint id"),
        },
        session_id: SessionId::try_from(SESSION_ID.to_owned()).expect("valid session id"),
    };

    let mut definitions = serde_json::Map::new();
    for method in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
        "ThreadQueueList",
    ] {
        definitions.insert(format!("{method}Params"), json!({"type":"object"}));
        definitions.insert(format!("{method}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))
            .expect("serialize native schema bundle"),
    )]))
    .expect("build native schema bundle");
    let schemas = Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)
            .expect("validate native schemas"),
    );
    let endpoint_observed_at = clock
        .utc_now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let description: EndpointDescription = serde_json::from_value(json!({
        "endpoint": target.endpoint,
        "label": "Wake fixture",
        "availability": {"state":"available","observedAt":endpoint_observed_at},
        "channels": [{
            "kind":"nativeCodex",
            "transport":"unixWebSocket",
            "path":"codex-native.sock",
            "schemaDigest": schemas.schema_digest(),
            "generation": generation
        }]
    }))
    .expect("valid endpoint description");
    let gate = NativeGenerationGate::default();
    gate.activate(generation, socket_path, Some(schemas))
        .expect("activate native generation");
    let native_backend = NativeControlBackend {
        endpoint: target.endpoint.clone(),
        gate,
        codex_home: root.path().to_owned(),
    };

    let anchor = initial_time - chrono::Duration::seconds(i64::from(REPEAT_INTERVAL_SECONDS) - 1);
    let expiry = anchor
        .checked_add_signed(chrono::Duration::seconds(
            i64::from(REPEAT_INTERVAL_SECONDS) * 2 + 3,
        ))
        .expect("wake expiry is in range");
    let message: SavedMessage = serde_json::from_value(json!({
        "target": target,
        "content": {
            "kind":"agent",
            "sender": target,
            "text": WAKE_BODY
        },
        "delivery":"queue",
        "generationGuard":null
    }))
    .expect("valid saved wake message");
    let wake = store
        .lock()
        .await
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message,
            timing: TimingRule::Interval {
                seconds: REPEAT_INTERVAL_SECONDS,
            },
            expiry: ExpiryRule::At { at: expiry },
            now_ms: anchor.timestamp_millis(),
        })
        .await
        .expect("create recurring wake");

    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
        .expect("create service identity")
        .with_machine_identity(
            MachineIdentity::new(service_identity.clone(), Some("wake-repeating-fixture"))
                .expect("create machine identity"),
        )
        .expect("attach machine identity")
        .with_endpoints(vec![description])
        .expect("add native endpoint")
        .with_automation_store(Arc::clone(&store))
        .with_runtime_clock(clock.clone())
        .with_interaction_store(Arc::clone(&interactions))
        .with_native_backend(native_backend.clone())
        .expect("add native backend");
    let route: Arc<dyn SessionDeliveryRoute> = Arc::new(CodexAppServerDeliveryRoute::new(
        clock.clone(),
        service_identity,
        identity.endpoint_directory(),
        native_backend,
        Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new()),
    ));
    let delivery: Arc<dyn SessionMessageDelivery> =
        Arc::new(SessionDeliveryRouter::new(vec![route]));
    let identity = identity.with_session_delivery(delivery);

    let (observation_sender, mut observation_receiver) = mpsc::channel(2);
    let native_server = tokio::spawn(serve_two_queue_deliveries(
        listener,
        String::from(target.session_id.clone()),
        observation_sender,
        clock.clone(),
    ));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let mut wake_worker = identity
        .wake_timing_worker()
        .expect("wake timing worker is available");
    let mut completions = wake_worker.observe_accepted_deliveries();
    let wake_observation = wake_worker.observation();
    let handoff_worker = tokio::spawn(
        identity
            .push_handoff_worker()
            .expect("push handoff worker is available")
            .run(clock.clone(), shutdown.clone()),
    );
    let worker = tokio::spawn(wake_worker.run(shutdown.clone()));
    drop(identity);

    let first_due_ms = anchor.timestamp_millis() + i64::from(REPEAT_INTERVAL_SECONDS) * 1000;
    asleep_with_timer_armed_for(&wake_observation, first_due_ms).await;
    clock
        .advance_to(runtime_primitives::SyncTime::from_unix_millis(first_due_ms))
        .unwrap();
    let first_delivery = next_native_queue_delivery(&mut observation_receiver)
        .await
        .expect("first native queue delivery arrives");
    let first_completion = (completions.recv())
        .await
        .expect("wake worker retains its completion sender");
    let next_due_at_ms = store
        .lock()
        .await
        .read_wakeup::<SavedMessage>(&wake.definition.wakeup_id)
        .await
        .expect("read wake after first acceptance commits")
        .next_due_at_ms;
    assert_eq!(
        next_due_at_ms,
        Some(first_due_ms + MINIMUM_REPEAT_GAP_MILLIS),
        "the stored next wake must be scheduled exactly 300 logical seconds after the first"
    );
    asleep_with_timer_armed_for(&wake_observation, first_due_ms + 300_000).await;
    clock
        .advance_to(runtime_primitives::SyncTime::from_unix_millis(
            first_due_ms + 300_000,
        ))
        .unwrap();
    let second_delivery = next_native_queue_delivery(&mut observation_receiver)
        .await
        .expect("second native queue delivery arrives");
    (async {
        loop {
            let completed = completions
                .recv()
                .await
                .expect("completion sender remains live");
            if completed != first_completion {
                break;
            }
        }
    })
    .await;
    assert!(
        second_delivery.observed_at_ms - first_delivery.observed_at_ms >= MINIMUM_REPEAT_GAP_MILLIS,
        "recurring wake gap must be at least 300 logical seconds"
    );
    (native_server)
        .await
        .expect("native server task succeeds")
        .expect("native server handles both deliveries");

    let (first_push_id, first_origin) = load_and_validate_stored_wake_push(
        &interactions,
        &first_delivery,
        &target,
        &wake.definition.wakeup_id,
    )
    .await
    .expect("first push is stored with its wake occurrence");
    let (second_push_id, second_origin) = load_and_validate_stored_wake_push(
        &interactions,
        &second_delivery,
        &target,
        &wake.definition.wakeup_id,
    )
    .await
    .expect("second push is stored with its wake occurrence");
    assert_ne!(
        first_push_id, second_push_id,
        "each firing needs a fresh PushId"
    );
    assert_ne!(
        first_origin, second_origin,
        "each firing needs a fresh origin ref"
    );
    let (
        RouterOriginRef::Wake {
            occurrence_id: first_occurrence,
            ..
        },
        RouterOriginRef::Wake {
            occurrence_id: second_occurrence,
            ..
        },
    ) = (&first_origin, &second_origin)
    else {
        panic!("stored push did not retain typed Wake origin facts");
    };
    assert_ne!(
        first_occurrence, second_occurrence,
        "recurring wake firings need distinct occurrence ids"
    );

    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
    )
    .await
    .expect("open mailbox database for verification");
    let stored_firings: Vec<(String, i64)> = sqlx::query_as(
        "SELECT occurrence_id,fired_at_ms FROM mailbox_deliveries WHERE wakeup_id=? ORDER BY due_at_ms,occurrence_id",
    )
    .bind(wake.definition.wakeup_id.as_str())
    .fetch_all(&mut connection)
    .await
    .expect("load stored wake occurrences");
    assert_eq!(stored_firings.len(), 2, "wake must fire exactly twice");
    assert!(
        stored_firings[1].1 - stored_firings[0].1 >= MINIMUM_REPEAT_GAP_MILLIS,
        "durable wake firing gap must be at least 300 logical seconds"
    );
    let stored_occurrences: Vec<String> = stored_firings.into_iter().map(|(id, _)| id).collect();
    let observed_occurrences = HashSet::from([
        String::from(first_occurrence.clone()),
        String::from(second_occurrence.clone()),
    ]);
    assert_eq!(stored_occurrences.len(), 2, "wake must fire exactly twice");
    assert_eq!(
        stored_occurrences.into_iter().collect::<HashSet<_>>(),
        observed_occurrences,
        "each stored mailbox occurrence must have its own delivered push"
    );
    connection.close().await.expect("close mailbox database");

    shutdown.cancel();
    (worker).await.expect("wake worker task succeeds");
    (handoff_worker).await.expect("push handoff task succeeds");
    Arc::try_unwrap(interactions)
        .unwrap_or_else(|_| panic!("wake interaction owner remained shared after shutdown"))
        .close()
        .await
        .expect("close interaction owner");
    let owned_store = Arc::try_unwrap(store)
        .map_err(|_| std::io::Error::other("wake store remained shared after worker shutdown"))
        .expect("release wake store after worker shutdown");
    owned_store
        .into_inner()
        .close()
        .await
        .expect("close automation store");
}

async fn next_native_queue_delivery(
    receiver: &mut mpsc::Receiver<NativeQueueObservation>,
) -> TestResult<NativeQueueObservation> {
    tokio::time::timeout(
        Duration::from_secs(u64::from(REPEAT_INTERVAL_SECONDS) + 8),
        receiver.recv(),
    )
    .await?
    .ok_or_else(|| "native server ended before both recurring deliveries arrived".into())
}

async fn load_and_validate_stored_wake_push(
    interactions: &Arc<interaction_storage::InteractionStore>,
    observed: &NativeQueueObservation,
    target: &SessionRef,
    wakeup_id: &WakeupId,
) -> TestResult<(PushId, RouterOriginRef)> {
    let push_id = PushId::try_from(observed.client_user_message_id.clone())?;
    let record = interactions
        .get_push_record(&push_id)
        .await?
        .ok_or("native delivery had no matching stored push")?;
    let origin_reference = RouterOriginRef::parse_canonical(
        record
            .origin_router_ref
            .as_deref()
            .ok_or("Wake push is missing its typed origin reference")?,
    )?;
    let RouterOriginRef::Wake {
        wakeup_id: stored_wakeup_id,
        ..
    } = &origin_reference
    else {
        return Err("stored push origin is not a wake".into());
    };
    if stored_wakeup_id != wakeup_id
        || record.push_id != push_id
        || record.kind != PushKind::Wake
        || record.origin != PushOrigin::Router(PushKind::Wake)
        || record.header_facts != PushHeaderFacts::Wake
        || &record.target != target
        || record.body.as_deref() != Some(WAKE_BODY)
        || observed.client_user_message_id.as_str() != record.push_id.as_str()
    {
        return Err("stored Wake push identity, target, or body diverged".into());
    }
    let link = observed
        .prompt_text
        .split_whitespace()
        .last()
        .and_then(|value| RouterLink::parse(value).ok())
        .ok_or("native prompt did not carry a Router push link")?;
    if !observed.prompt_text.starts_with("⏰ Router wake @")
        || observed.prompt_text.contains('\n')
        || link.push_id() != &record.push_id
        || link.push_id().as_str() != observed.client_user_message_id.as_str()
    {
        return Err(
            "native prompt link and clientUserMessageId must match the stored PushId".into(),
        );
    }
    Ok((record.push_id, origin_reference))
}

async fn serve_two_queue_deliveries(
    listener: tokio::net::UnixListener,
    expected_thread_id: String,
    observations: mpsc::Sender<NativeQueueObservation>,
    clock: Arc<dyn router_clock::RuntimeClock>,
) -> TestResult<()> {
    for delivery_number in 1..=2 {
        let (stream, _) = tokio::time::timeout(
            Duration::from_secs(u64::from(REPEAT_INTERVAL_SECONDS) + 8),
            listener.accept(),
        )
        .await??;
        let mut socket = accept_async(stream).await?;
        let initialize = next_json_frame(&mut socket).await?;
        send_json_frame(&mut socket, json!({"id":initialize.get("id"),"result":{}})).await?;
        let initialized = next_json_frame(&mut socket).await?;
        if initialized.pointer("/method").and_then(Value::as_str) != Some("initialized") {
            return Err("native client skipped its initialized notification".into());
        }
        let read = next_json_frame(&mut socket).await?;
        if read.pointer("/method").and_then(Value::as_str) != Some("thread/read")
            || read.pointer("/params/threadId").and_then(Value::as_str)
                != Some(expected_thread_id.as_str())
        {
            return Err("wake route did not check native residency for the target thread".into());
        }
        send_json_frame(
            &mut socket,
            json!({
                "id":read.get("id"),
                "result":{"thread":{"id":expected_thread_id,"status":{"type":"idle"}}}
            }),
        )
        .await?;
        let queued = next_json_frame(&mut socket).await?;
        if queued.pointer("/method").and_then(Value::as_str) != Some("thread/queue/add")
            || queued.pointer("/params/threadId").and_then(Value::as_str)
                != Some(expected_thread_id.as_str())
        {
            return Err("wake queue delivery changed its native target or mode".into());
        }
        let client_user_message_id = queued
            .pointer("/params/clientUserMessageId")
            .and_then(Value::as_str)
            .ok_or("native queue request is missing clientUserMessageId")?
            .to_owned();
        let prompt_text = queued
            .pointer("/params/input/0/text")
            .and_then(Value::as_str)
            .ok_or("native queue request is missing the rendered push line")?
            .to_owned();
        let queue_request_id = queued
            .get("id")
            .cloned()
            .ok_or("native queue request is missing its RPC id")?;
        send_json_frame(
            &mut socket,
            json!({
                "id":queue_request_id,
                "result":{"queuedSubmission":{"id":format!("fixture-queue-{delivery_number}")}}
            }),
        )
        .await?;
        observations
            .send(NativeQueueObservation {
                client_user_message_id,
                prompt_text,
                observed_at_ms: clock.utc_now().timestamp_millis(),
            })
            .await
            .map_err(|_| "test stopped reading native queue observations")?;
    }
    Ok(())
}

async fn next_json_frame(
    socket: &mut WebSocketStream<tokio::net::UnixStream>,
) -> TestResult<Value> {
    let frame = tokio::time::timeout(Duration::from_secs(4), socket.next())
        .await?
        .ok_or("native WebSocket closed before the next frame")??;
    Ok(serde_json::from_str(frame.to_text()?)?)
}

async fn send_json_frame(
    socket: &mut WebSocketStream<tokio::net::UnixStream>,
    value: Value,
) -> TestResult<()> {
    socket.send(Message::Text(value.to_string().into())).await?;
    Ok(())
}
