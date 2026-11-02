//! Central preparation through real SDK, central/Router SQLite and scripted native WebSockets.
use collaboration_protocol::{
    CodexGeneration, EndpointDescription, InstructionCreateParams, InstructionText, OperationId,
    ScheduleCreateRequest, SchedulePrepareRequest, SessionRef,
};
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_service::ApplicationFixture;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
#[path = "support/served_api.rs"]
mod served_api;
#[tokio::test]
async fn preparation_replay_uses_recorded_native_thread_without_forking_again()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_preparation(false, false, false).await
}
#[tokio::test]
async fn lost_preparation_response_is_retained_without_reallocation()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_preparation(true, false, false).await
}
#[tokio::test]
async fn ownership_rejection_allows_preparing_a_different_thread()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_preparation(false, true, false).await
}
#[tokio::test]
async fn ordinary_edit_during_preparation_allows_a_new_preparation()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_preparation(false, false, true).await
}
async fn exercise_preparation(
    lose_response: bool,
    ownership_conflict: bool,
    change_conflict: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fixture = fixture::CatalogFixture::open(|transport| transport, true).await?;
    let proof: fixture::ProofResult = async {
    let root = fixture.api.directory().to_path_buf();
    let store = fixture.store.as_ref().ok_or("missing Router store")?.clone();
    let path = root.join("native.sock");
    let listener = tokio::net::UnixListener::bind(&path)?;
    let service_id = "00000000-0000-4000-8000-000000000001";
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service_id,"generation":1}))?;
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"fixture-new-target"}),
    )?;
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
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
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
        &bundle,
    )?);
    let description: EndpointDescription = serde_json::from_value(
        json!({"endpoint":target.endpoint,"label":"Fixture","availability":{"state":"available","observedAt":"2026-09-08T00:00:00Z"},"channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock","schemaDigest":schemas.schema_digest(),"generation":generation}]}),
    )?;
    let gate = NativeGenerationGate::default();
    gate.activate(generation, path.clone(), Some(schemas))?;
    let native_backend = NativeControlBackend {
        endpoint: target.endpoint.clone(),
        gate,
        codex_home: root.clone(),
    };
    let identity = ApplicationFixture::new(service_id, service_id)?
        .with_endpoints(vec![description])?
        .with_catalog(Some(fixture.transport.clone()))
        .with_automation_store(store.clone())
        .with_scheduled_run_execution(Arc::new(
            collaboration_runtime::CodexAppServerScheduledRuns::new(
                native_backend.clone(),
                Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new()),
            ),
        ))
        .with_native_backend(native_backend)?;
    let served = served_api::ServedApi::start(identity.clone())?;
    let client = served.client("prepare-fixture").await?;
    if !matches!(fixture.transport.call(collaboration_protocol::AutomationCall::PullAutomationBatches(
        collaboration_protocol::PullAutomationBatches {
            after: None, max_bytes: 1_048_576,
            work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
        }
    )).await?, collaboration_protocol::AutomationAnswer::Batches(_)) {
        return Err("preparation target did not advertise MachineWorkV1".into());
    }
    let instruction = client
        .create_instruction(InstructionCreateParams {
            operation_id: OperationId::generate(),
            text: InstructionText::try_from("Check status".to_owned())?,
        })
        .await?;
    let schedule=client.create_schedule(serde_json::from_value::<ScheduleCreateRequest>(json!({"operationId":OperationId::generate(),"definition":{"instructionId":instruction.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":"gpt-5.6-sol","effort":"medium"}}))?).await?;
    if ownership_conflict {
        // A retained Local Schedule's real native claim must still exclude a central binding.
        let local_instruction = store.lock().await.create_instruction(
            &OperationId::generate(), &"Retained Local owner".to_owned().try_into()?, 0,
        ).await?;
        let owner = store.lock().await.create_schedule(&automation_storage::ScheduleCreate::<SessionRef, collaboration_protocol::EndpointRef> {
            operation_id: OperationId::generate(),
            definition: serde_json::from_value(json!({"instructionId":local_instruction.instruction_id,"timing":{"kind":"interval","seconds":600},"enabled":false,"destination":{"kind":"unprepared"},"executionTimeoutSeconds":null,"model":"gpt-5.6-sol","effort":"medium"}))?,
            imported_continuity: agent_automation::ContinuityInput::None,
            now_ms: 0,
        }).await?;
        store
            .lock()
            .await
            .claim_thread_binding(&automation_storage::ThreadBindingClaim {
                schedule_id: owner.schedule_id,
                service_id: service_id.into(),
                endpoint_id: "codex-local".into(),
                thread_id: "owned-thread".into(),
                now_ms: 0,
            })
            .await?;
    }
    let existing_conflict = ownership_conflict || change_conflict;
    let backend_client = fixture.api.client("edit-during-native-preparation").await?;
    let backend_schedule = schedule.schedule_id.clone();
    let backend = tokio::spawn(async move {
        for attempt in 0..if existing_conflict { 2 } else { 1 } {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let init: Value =
                serde_json::from_str(socket.next().await.ok_or("missing init")??.to_text()?)?;
            socket
                .send(Message::Text(
                    json!({"id":init.get("id"),"result":{}}).to_string().into(),
                ))
                .await?;
            let _initialized = socket.next().await.ok_or("missing initialized")??;
            let request: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .ok_or("missing thread start")??
                    .to_text()?,
            )?;
            if change_conflict && attempt == 0 {
                let collaboration_client::ScheduleReadResponse::Schedule {value: current} = backend_client
                    .read_schedule(collaboration_protocol::ScheduleShowRequest {schedule_id: backend_schedule.clone()}).await?
                    else { return Err("central Schedule unavailable during native preparation".into()); };
                let mut definition = current.definition;
                definition.execution_timeout_seconds = Some(120.try_into()?);
                backend_client
                    .update_schedule(collaboration_protocol::ScheduleUpdateRequest {
                        operation_id: OperationId::generate(),
                        schedule_id: backend_schedule.clone(),
                        expected_change_id: current.change_id,
                        definition,
                    })
                    .await?;
            }
            if existing_conflict {
                if request.get("method").and_then(Value::as_str) != Some("thread/read") {
                    return Err("existing preparation must only read native thread".into());
                }
                socket.send(Message::Text(json!({"id":request.get("id"),"result":{"thread":{"id":request.pointer("/params/threadId"),"cwd":"/fresh-fixture"}}}).to_string().into())).await?;
            } else {
                if request.get("method").and_then(Value::as_str) != Some("thread/start")
                    || request.pointer("/params/cwd").and_then(Value::as_str)
                        != Some("/fresh-fixture")
                    || request.pointer("/params/model").and_then(Value::as_str)
                        != Some("gpt-5.6-sol")
                    || request
                        .pointer("/params/config/model_reasoning_effort")
                        .and_then(Value::as_str)
                        != Some("medium")
                {
                    return Err("wrong preparation call or cwd".into());
                }
                if !lose_response {
                    socket.send(Message::Text(json!({"id":request.get("id"),"result":{"thread":{"id":"prepared-new-thread","cwd":"/fresh-fixture"},"cwd":"/fresh-fixture","model":"gpt-5.6-sol"}}).to_string().into())).await?;
                }
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let destination = if existing_conflict {
        json!({"kind":"existing","target":{"endpoint":target.endpoint,"sessionId":"owned-thread"},"cwd":"/fresh-fixture"})
    } else {
        json!({"kind":"fresh","endpoint":target.endpoint,"cwd":"/fresh-fixture"})
    };
    let request: SchedulePrepareRequest = serde_json::from_value(
        json!({"operationId":OperationId::generate(),"scheduleId":schedule.schedule_id,"destination":destination}),
    )?;
    let first = tokio::time::timeout(
        Duration::from_secs(3),
        client.prepare_schedule(request.clone()),
    )
    .await?;
    if existing_conflict {
        let error = match first {
            Err(collaboration_client::ScheduleClientError::Rejected(error)) => error,
            _ => return Err("ownership conflict did not return a typed rejection".into()),
        };
        let correct_error = if change_conflict {
            matches!(
                error.kind,
                collaboration_protocol::ScheduleFailureKind::ChangeConflict
            ) && matches!(
                error.next_action,
                collaboration_protocol::ScheduleNextAction::InspectSchedule
            )
        } else {
            matches!(
                error.kind,
                collaboration_protocol::ScheduleFailureKind::OwnershipConflict
            ) && matches!(
                error.next_action,
                collaboration_protocol::ScheduleNextAction::SelectDifferentThread
            )
        };
        if !correct_error {
            return Err("known preparation rejection became unrecoverable uncertainty".into());
        }
        let collaboration_client::ScheduleReadResponse::Schedule { value: unchanged } = client
            .read_schedule(collaboration_protocol::ScheduleShowRequest {schedule_id: schedule.schedule_id.clone()}).await?
            else {return Err("refused central binding lost its Schedule".into());};
        if !matches!(unchanged.definition.destination, collaboration_protocol::ExecutionDestination::Unprepared)
            || (change_conflict && error.current_change_id != Some(unchanged.change_id)) {
            return Err("refused binding changed central destination or omitted actual conflicting revision".into());
        }
        let served_retry_client = served_api::ServedApi::start(identity.clone())?;
        let retry_client = served_retry_client.client("ownership-recovery").await?;
        let retry = retry_client.prepare_schedule(serde_json::from_value(json!({
            "operationId":OperationId::generate(),"scheduleId":schedule.schedule_id,
            "destination":{"kind":"existing","target":{"endpoint":target.endpoint,"sessionId":"different-thread"},"cwd":"/fresh-fixture"}
        }))?).await?;
        if !matches!(retry.definition.destination, collaboration_protocol::ExecutionDestination::OwnedThread { target, .. } if String::from(target.session_id.clone()) == "different-thread")
        {
            return Err("different thread could not be prepared after known rejection".into());
        }
        served_retry_client.stop().await?;
    } else if lose_response {
        let error = match first {
            Err(collaboration_client::ScheduleClientError::Rejected(error)) => error,
            _ => return Err("lost allocation response lacked typed evidence".into()),
        };
        if !matches!(&error.effects,collaboration_protocol::ScheduleEffects::Route{evidence} if matches!(evidence.as_ref(), collaboration_protocol::DeliveryRouteEvidence::CodexAppServer(native) if matches!(native.allocation,collaboration_protocol::PreparationEffect::Unknown)))
        {
            return Err("lost allocation response invented no effect".into());
        }
        let served_replay_client = served_api::ServedApi::start(identity)?;
        let replay_client = served_replay_client.client("prepare-replay").await?;
        let replay = replay_client.prepare_schedule(request).await;
        let replay = match replay {
            Err(collaboration_client::ScheduleClientError::Rejected(error)) => error,
            _ => return Err("uncertain preparation replayed allocation".into()),
        };
        if serde_json::to_value(error)? != serde_json::to_value(replay)? {
            return Err("retained preparation failure changed on replay".into());
        }
        served_replay_client.stop().await?;
    } else {
        let first = first?;
        let replay = client.prepare_schedule(request).await?;
        if serde_json::to_value(&first)? != serde_json::to_value(replay)?
            || first.definition.enabled
        {
            return Err("preparation replay or activation state changed".into());
        }
        match first.definition.destination {
            collaboration_protocol::ExecutionDestination::OwnedThread { target, .. }
                if String::from(target.session_id.clone()) == "prepared-new-thread" => {}
            _ => return Err("native thread identity not bound".into()),
        }
    }
    let collaboration_protocol::AutomationAnswer::Schedule(central) = fixture.transport
        .call(collaboration_protocol::AutomationCall::ScheduleShow(collaboration_protocol::ScheduleShowRequest {schedule_id: schedule.schedule_id.clone()})).await?
        else {return Err("central Schedule not inspectable after native preparation".into());};
    let expected_thread = if existing_conflict {"different-thread"} else {"prepared-new-thread"};
    if central.definition.enabled
        || if lose_response {
            !matches!(central.definition.destination, collaboration_protocol::ExecutionDestination::Unprepared)
                || central.change_id != schedule.change_id
        } else {
            !matches!(central.definition.destination, collaboration_protocol::ExecutionDestination::OwnedThread {target,..} if String::from(target.session_id.clone())==expected_thread)
                || central.change_id == schedule.change_id
        }
    {
        return Err("native preparation result disagreed with actual central binding/revision".into());
    }
    served.stop().await?;
    backend.await??;
    drop(store);
    Ok(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed
}
