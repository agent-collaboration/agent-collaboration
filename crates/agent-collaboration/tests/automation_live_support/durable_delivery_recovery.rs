//! Retained Local delivery recovery with a controlled gate; package import uses real central.
//! Host discovery and native relay publication have separate live acceptance coverage.
use super::{
    proof_context::{ProofContext, ProofResult},
    summary_failure_recovery::PortableRunProof,
};
use automation_storage::AutomationStore;
use collaboration_client::WakeWaitError;
use collaboration_client::protocol::{
    ChannelDescription, CodexGeneration, DeliveryEvidence, DeliveryListRequest,
    DeliveryShowRequest, EndpointAvailability, EndpointDescription, EndpointRef,
    ExecutionDestination, ExpiryRequest, ImportedContinuity, MessageContent, MessageDelivery,
    NativeCarrier, OperationId, RunListRequest, SavedMessage, ScheduleImportRequest, SessionRef,
    TimingRequest, WakeMutationRequest, WakeSendRequest, WakeShowRequest,
};
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{os::unix::fs::DirBuilderExt, sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub async fn exercise(proof: &mut ProofContext, portable: PortableRunProof) -> ProofResult<()> {
    let native_target = proof
        .start_thread("Luna durable delivery recipient")
        .await?;
    let directory = proof.root.join("recovery-service");
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&directory.join("automation.sqlite")).await?,
    ));
    let service_id = uuid::Uuid::now_v7().to_string();
    let epoch = uuid::Uuid::now_v7().to_string();
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":epoch,"generation":1}))?;
    let endpoint = EndpointRef {
        service_id: service_id.clone().try_into()?,
        endpoint_id: proof.endpoint.endpoint_id.clone(),
    };
    let target = SessionRef {
        endpoint: endpoint.clone(),
        session_id: native_target.session_id,
    };
    let mut description = EndpointDescription {
        endpoint: endpoint.clone(),
        label: "Recovery acceptance".to_owned().try_into()?,
        availability: EndpointAvailability::Unavailable {
            observed_at: timestamp()?,
            reason: "Backend admission intentionally held by the acceptance test"
                .to_owned()
                .try_into()?,
            fix: None,
        },
        channels: vec![ChannelDescription::NativeCodex {
            transport: NativeCarrier::UnixWebSocket,
            path: "codex-native.sock".to_owned().try_into()?,
            schema_digest: Some(proof.schemas.schema_digest().to_owned().try_into()?),
            generation: None,
        }],
    };
    let gate = NativeGenerationGate::default();
    let central = board_central_test_support::TestCentral::start_with_peer(
        &directory.join("test-central"),
        &message_board::ActorOrigin {
            installation: service_id.clone().try_into()?,
            machine: "retained delivery recovery".to_owned().try_into()?,
        },
        uuid::Uuid::now_v7().to_string().try_into()?,
    )
    .await?;
    let (requests, central_runtime) = central.connect_machine_session(&directory).await?;
    let catalog: Arc<dyn collaboration_runtime::CentralAutomationTransport> = Arc::new(
        collaboration_runtime::SessionAutomationTransport::new(requests),
    );
    let identity = ApplicationFixture::new(&service_id, &epoch)?
        .with_catalog(Some(catalog.clone()))
        .with_endpoints(vec![description.clone()])?
        .with_automation_store(Arc::clone(&store))
        .with_native_backend(NativeControlBackend {
            endpoint,
            gate: gate.clone(),
            codex_home: proof.owned_codex_home().to_path_buf(),
        })?;
    let served = match ServedCollaborationApi::start(&directory, identity.clone().build()) {
        Ok(served) => served,
        Err(error) => {
            central_runtime.shutdown().await?;
            return Err(error.into());
        }
    };
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = shutdown.clone().drop_guard();
    let mut worker = None;
    let scenario:ProofResult<()>=async {
    let client = served.client("durable-recovery-proof").await?;
    let imported = client
        .import_schedule(ScheduleImportRequest {
            operation_id: OperationId::generate(),
            package_utf8: portable.package_utf8,
            overwrite: false,
        })
        .await?;
    if !imported.already_sent.is_empty(){return Err("disabled central import invented sent work".into());}
    let collaboration_client::CatalogAutomationResult::Schedule(imported)=imported.automation else{return Err("central import returned another automation kind".into());};
    if imported.schedule_id != portable.schedule_id
        || imported.change_id == portable.source_change_id
        || imported.definition.enabled
        || !matches!(
            imported.definition.destination,
            ExecutionDestination::FreshEachRunUnprepared
        )
        || !matches!(&imported.imported_continuity, ImportedContinuity::ImportedSummary { text, .. } if text == &portable.summary_text)
    {
        return Err("Portable import did not preserve identity, summary and fresh execution mode with a new edit token and disabled unprepared bindings".into());
    }
    let runs = client
        .list_runs(RunListRequest {
            schedule_id: imported.schedule_id.clone(),
            cursor: None,
            limit: 100.try_into()?,
        })
        .await?;
    if !runs.records.is_empty() || runs.next_cursor.is_some() || gate.acquire().is_ok() {
        return Err("Import created execution or unexpectedly opened native admission".into());
    }
    proof.record("disabledImportWithRealSummaryVerified", json!(imported))?;
    worker=Some(tokio::spawn(
        identity
            .wake_timing_worker()
            .ok_or("Wake worker missing")?
            .run(shutdown.clone()),
    ));
    let marker = format!("DELIVERY_RECOVERED_{}", OperationId::generate().as_str());
    let request = WakeSendRequest {
        requested_by: collaboration_client::protocol::WakeCreator::Owner {},
        replace: false,
        operation_id: OperationId::generate(),
        message: SavedMessage {
            target: target.clone(),
            content: MessageContent::Agent {
                sender: target.clone(),
                text: format!("Output exactly {marker}. Do not call tools or spawn agents.")
                    .try_into()?,
            },
            delivery: MessageDelivery::Auto,
            generation_guard: None,
        },
        timing: TimingRequest::After {
            seconds: 1.try_into()?,
        },
        expiry: ExpiryRequest::None,
    };
    let wake=seed_legacy_wake(&store,&client,request.clone()).await?;
    let waiter = served
        .client("durable-recovery-waiter")
        .await?
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await?;
    let fire =
        tokio::time::timeout(Duration::from_secs(10), waiter.wait_until_first_fire()).await??;
    let collaboration_client::WakeFirstFireResponse::Local {value:fire}=fire else{return Err("retained Local first-fire wait changed owner".into());};
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let pending = loop {
        interval.tick().await;
        let deliveries = client
            .list_deliveries(DeliveryListRequest {
                wakeup_id: Some(wake.definition.wakeup_id.clone()),
                cursor: None,
                limit: 100.try_into()?,
            })
            .await?;
        if deliveries.records.len() > 1 || deliveries.next_cursor.is_some() {
            return Err("One firing created duplicate delivery obligations".into());
        }
        if let Some(delivery) = deliveries.records.into_iter().next() {
            match &delivery.evidence {
                DeliveryEvidence::KnownNotSubmitted { .. } => break delivery,
                DeliveryEvidence::Accepted { .. } | DeliveryEvidence::OutcomeUnknown { .. } => {
                    return Err(
                        "Closed native admission unexpectedly acquired external effects".into(),
                    );
                }
                _ => {}
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "No known non-submission evidence while backend admission was closed".into(),
            );
        }
    };
    if gate.acquire().is_ok() || fire.wakeup_id != wake.definition.wakeup_id {
        return Err("First-fire proof did not occur under closed backend admission".into());
    }
    proof.record(
        "firstFireBeforeNativeAcceptanceVerified",
        json!({"fire":fire,"delivery":pending}),
    )?;
    gate.activate(
        generation.clone(),
        proof.root.join("native-socket/app-server.sock"),
        Some(Arc::clone(&proof.schemas)),
    )?;
    description.availability = EndpointAvailability::Available {
        observed_at: timestamp()?,
    };
    for channel in &mut description.channels {
        if let ChannelDescription::NativeCodex {
            generation: published,
            ..
        } = channel
        {
            *published = Some(generation.clone());
        }
    }
    identity
        .endpoint_directory()
        .publish(description)
        .map_err(|error| error.to_string())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let accepted = loop {
        interval.tick().await;
        let delivery = client
            .read_delivery(DeliveryShowRequest {
                delivery_id: pending.delivery_id.clone(),
            })
            .await?;
        if matches!(delivery.evidence, DeliveryEvidence::Accepted { .. }) {
            break delivery;
        }
        if matches!(delivery.evidence, DeliveryEvidence::OutcomeUnknown { .. }) {
            return Err("Recovered backend delivery is uncertain; input must not be resent".into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "Same delivery did not obtain native acceptance after admission opened".into(),
            );
        }
    };
    let turns = proof.wait_for_text(&target, &marker).await?;
    let count = turns
        .iter()
        .filter_map(|turn| turn.get("items").and_then(Value::as_array))
        .flatten()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("userMessage")
                && item.get("clientId").and_then(Value::as_str)
                    == Some(pending.delivery_id.as_str())
        })
        .count();
    if count != 1 {
        return Err(
            "Recovered delivery did not produce exactly one correlated native input".into(),
        );
    }
    proof.record(
        "sameDeliveryRecoveredToRealLuna",
        json!({"fire":fire,"delivery":accepted,"nativeInputCount":count}),
    )?;
    let mut paused_request = request;
    paused_request.operation_id = OperationId::generate();
    paused_request.timing = TimingRequest::After {
        seconds: 60.try_into()?,
    };
    let paused=seed_legacy_wake(&store,&client,paused_request).await?;
    let waiter = served
        .client("durable-recovery-waiter")
        .await?
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: paused.definition.wakeup_id.clone(),
        })
        .await?;
    client
        .pause_wakeup(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: paused.definition.wakeup_id.clone(),
        })
        .await?;
    if !matches!(tokio::time::timeout(Duration::from_secs(5), waiter.wait_until_first_fire()).await?,
        Err(WakeWaitError::Paused { wakeup_id }) if wakeup_id == paused.definition.wakeup_id)
    {
        return Err("Pause before first firing did not produce a typed wait error".into());
    }
    client
        .cancel_wakeup(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: paused.definition.wakeup_id.clone(),
        })
        .await?;
    proof.record(
        "pausedFirstFireWaitRejected",
        json!({"wakeupId":paused.definition.wakeup_id}),
    )?;
    Ok(())
    }.await;
    shutdown.cancel();
    let worker_stop: ProofResult<()> = async {
        if let Some(worker) = worker {
            worker.await?;
        }
        Ok(())
    }
    .await;
    let api_stop = served.stop().await;
    drop(identity);
    drop(catalog);
    let central_stop = central_runtime.shutdown().await;
    let storage_stop: ProofResult<()> = async {
        Arc::try_unwrap(store)
            .map_err(|_| "durable recovery still owns local storage")?
            .into_inner()
            .close()
            .await?;
        Ok(())
    }
    .await;
    match (scenario, worker_stop, api_stop, central_stop, storage_stop) {
        (Ok(()), Ok(()), Ok(()), Ok(()), Ok(())) => Ok(()),
        results => Err(format!(
            "durable recovery scenario/worker/API/central/storage results: {results:?}"
        )
        .into()),
    }
}

fn timestamp() -> ProofResult<collaboration_client::protocol::ObservationTimestamp> {
    Ok(chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .try_into()?)
}

async fn seed_legacy_wake(
    store: &Arc<Mutex<AutomationStore>>,
    client: &collaboration_client::CollaborationClient,
    request: WakeSendRequest,
) -> ProofResult<collaboration_protocol::WakeSnapshot> {
    // Actual pre-upgrade owner state; new public Wake creation belongs to central.
    let id = store
        .lock()
        .await
        .create_wakeup(&automation_storage::WakeCreate {
            operation_id: request.operation_id,
            requested_by: request.requested_by,
            replace: request.replace,
            message: request.message,
            timing: serde_json::from_value(serde_json::to_value(request.timing)?)?,
            expiry: serde_json::from_value(serde_json::to_value(request.expiry)?)?,
            now_ms: chrono::Utc::now().timestamp_millis(),
        })
        .await?
        .definition
        .wakeup_id;
    match client
        .read_wakeup(WakeShowRequest { wakeup_id: id })
        .await?
    {
        collaboration_client::WakeReadResponse::Local { value } => Ok(value),
        _ => Err("pre-upgrade recovery Wake did not retain Local ownership".into()),
    }
}
