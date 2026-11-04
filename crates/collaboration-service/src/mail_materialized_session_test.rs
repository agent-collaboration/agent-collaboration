//! Real native protocol, reader actor and SDK custody; the external native server stands in.
use crate::mail_delivery_projection_tests::fixture::{Fixture, ProjectionTask};
use crate::{
    BoardAvailability, MailPushDelivery, MailPushDeliveryProps, SubscriptionDeliveryService,
    SubscriptionDeliveryServiceProps, SystemSubscriptionClock,
};
use collaboration_contracts::{MachineIdentity, SessionDeliveryRoute};
use collaboration_protocol::{CodexGeneration, MessageDelivery, PushDeliveryState, SessionRef};
use collaboration_runtime::{
    CodexAppServerDeliveryRoute, EndpointDirectory, NativeControlBackend, NativeGenerationGate,
    SessionDeliveryRouter, UnmaterializedThreadHolder,
};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::Inbox;
use fleet_sync_protocol::{RecipientOutcome, SyncTime};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn materialized_unloaded_session_holds_mail_then_delivers_after_readiness() {
    // Arrange: successful native metadata for a persisted thread, distinct from
    // the -32600 missing-thread error of an empty thread destroyed on unload.
    let fixture = Fixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let mut draft = fixture.draft();
    draft.target.session_id = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    draft.mode = Some(MessageDelivery::Queue);
    let item = fixture.item(&draft);
    let item_id = item.id();
    let push_id = draft.push_id.clone();
    let target = draft.target.clone();
    let machine =
        MachineIdentity::new(target.endpoint.service_id.clone(), Some("native proof")).unwrap();
    let loaded = Arc::new(AtomicBool::new(false));
    let inputs = Arc::new(AtomicUsize::new(0));
    let route = native_route(&target, socket, root.path());
    let delivery = Arc::new(SessionDeliveryRouter::new(vec![route]));
    let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: fixture.owner.clone(),
        delivery: delivery.clone(),
        presence: delivery.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(SystemSubscriptionClock),
    });
    actors.start().await.unwrap();
    *fixture.host.consumer.native.lock().unwrap() =
        Some(Arc::new(MailPushDelivery::new(MailPushDeliveryProps {
            interactions: fixture.owner.clone(),
            actors: actors.clone(),
            presence: delivery,
            machine,
            clock: Arc::new(SystemSubscriptionClock),
        })));
    let server = tokio::spawn(serve_materialized_thread(
        listener,
        target,
        push_id.as_str().to_owned(),
        loaded.clone(),
        inputs.clone(),
    ));

    // Act: the real Inbox admission and journal task inspect the native thread.
    assert!(fixture.admit_local(draft).await.unwrap());
    let engine = fixture.engine();
    engine.drive_due().await.unwrap();

    // Assert: no input or resume while unloaded; custody and public projection hold.
    assert!(matches!(
        fixture.local_outcome(item_id).await,
        Some(RecipientOutcome::Held)
    ));
    assert_eq!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    assert_eq!(inputs.load(Ordering::SeqCst), 0);

    // Act: the protocol now reports the same thread loaded. Inject the existing
    // SDK readiness release, using this fixture's one physical owner and clock.
    loaded.store(true, Ordering::SeqCst);
    release_ready_item(&fixture, item_id, push_id.clone()).await;
    engine.drive_due().await.unwrap();

    // Assert: the original item is delivered once, with its frozen correlation.
    assert!(matches!(
        fixture.local_outcome(item_id).await,
        Some(RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Delivered
    );
    assert_eq!(inputs.load(Ordering::SeqCst), 1);
    engine.drive_due().await.unwrap();
    assert_eq!(inputs.load(Ordering::SeqCst), 1);
    server.await.unwrap();
    actors.shutdown().await;
    *fixture.host.consumer.native.lock().unwrap() = None;
    drop(actors);
    drop(engine);
    drop(fixture.close().await);
}

async fn release_ready_item(
    fixture: &Fixture,
    item: fleet_sync_protocol::ItemId,
    push: collaboration_protocol::PushId,
) {
    let config = fixture.host.config;
    let binding = fixture.host.binding.clone();
    let now = SyncTime::from_unix_millis(fixture.host.clock.load(Ordering::SeqCst));
    fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                assert!(
                    interaction_storage::mark_released_mail_pending_in_scope(scope, &push)
                        .await
                        .map_err(StoreError::backend)?
                );
                assert!(
                    Inbox::<_, collaboration_contracts::RouterDeliveryTypes, _>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                        binding.as_ref(),
                    )
                    .release_held::<ProjectionTask>(item, now)
                    .await
                    .map_err(StoreError::backend)?
                    .is_some()
                );
                Ok(())
            })
        })
        .await
        .unwrap();
}

fn native_route(
    target: &SessionRef,
    socket: std::path::PathBuf,
    home: &std::path::Path,
) -> Arc<dyn SessionDeliveryRoute> {
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "ThreadTurnsList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).unwrap(),
    )]))
    .unwrap();
    let schemas =
        Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(&bundle).unwrap());
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch":uuid::Uuid::now_v7().to_string(),"generation":1
    }))
    .unwrap();
    let endpoints = EndpointDirectory::new(target.endpoint.service_id.clone());
    endpoints
        .publish(
            serde_json::from_value(json!({
                "endpoint":target.endpoint,"label":"Materialized native fixture",
                "availability":{"state":"available","observedAt":"2026-11-03T00:00:00Z"},
                "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock",
                    "schemaDigest":schemas.schema_digest(),"generation":generation}]
            }))
            .unwrap(),
        )
        .unwrap();
    let gate = NativeGenerationGate::default();
    gate.activate(generation, socket, Some(schemas)).unwrap();
    Arc::new(CodexAppServerDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        endpoints,
        NativeControlBackend {
            endpoint: target.endpoint.clone(),
            gate,
            codex_home: home.to_owned(),
        },
        Arc::new(UnmaterializedThreadHolder::new()),
    ))
}

async fn serve_materialized_thread(
    listener: tokio::net::UnixListener,
    target: SessionRef,
    push_id: String,
    loaded: Arc<AtomicBool>,
    inputs: Arc<AtomicUsize>,
) {
    // Three connections: unloaded presence, loaded presence, actual delivery.
    for _ in 0..3 {
        let (stream, _) = listener.accept().await.unwrap();
        let mut wire = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(frame) = wire.next().await {
            let frame = match frame {
                Ok(Message::Text(text)) => text,
                _ => break,
            };
            let request: Value = serde_json::from_str(&frame).unwrap();
            let method = request["method"].as_str().unwrap();
            if method == "initialized" {
                continue;
            }
            let result = match method {
                "initialize" => json!({}),
                "thread/read" => {
                    assert_eq!(request["params"]["threadId"], json!(target.session_id));
                    assert_eq!(request["params"]["includeTurns"], json!(false));
                    json!({"thread":{"id":target.session_id,"cwd":"/work",
                        "status":{"type":if loaded.load(Ordering::SeqCst) {"idle"} else {"notLoaded"}}}})
                }
                "thread/queue/add" => {
                    assert!(loaded.load(Ordering::SeqCst));
                    assert_eq!(request["params"]["threadId"], json!(target.session_id));
                    assert_eq!(request["params"]["clientUserMessageId"], json!(push_id));
                    inputs.fetch_add(1, Ordering::SeqCst);
                    json!({"queuedSubmission":{"id":uuid::Uuid::now_v7().to_string()}})
                }
                unexpected => panic!("unloaded custody must not start or resume: {unexpected}"),
            };
            wire.send(Message::Text(
                json!({"id":request["id"],"result":result})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        }
    }
}
