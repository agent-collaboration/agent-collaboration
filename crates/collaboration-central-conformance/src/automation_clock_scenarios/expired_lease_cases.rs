//! A populated normal Registry Welcome witnesses expiry before the Fleet admits work.
use super::{AutomationClockPlatform, call, pull, require_instruction};
use crate::{
    AutomationClockControl, ClockObservation, ConformanceClient, ConformanceResult, RuntimeControl,
    enroll_admitted_peer, native_board_session::NativeBoardSession,
};
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationCall, ExecutionDestination, GridTime,
    InstructionCreateParams, MachineWorkProtocol, PullAutomationBatches, ScheduleCreateRequest,
    ScheduleDefinition, TimingRequest, WorkEntry,
};
use fleet_admission::{FleetOperation, FleetRequest};
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, DatabaseId, DatabaseStatus, Epoch, LeaseAnswer, LeaseRequest,
    LeaseState, OperationAnswer, OperationRequest, Origin, PeerId, Piece, PieceContents,
    Registration, RegistrationAnswer, ReplicatedTransaction, ReplicationAnswer,
    ReplicationPosition, ReplicationRequest, RequestId, SchemaSupport, ServerFrame, SessionId,
};
use futures_util::{SinkExt, StreamExt};
use message_board::{
    BoardId, Description, ProjectId, RecordedActor, RequestDecision, ResourceName, WorkspaceId,
};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};

pub(super) async fn run(
    client: &ConformanceClient,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    let base = RegistryTime("2030-01-01T00:00:00Z".parse()?);
    assert_eq!(
        client
            .automation_clock(AutomationClockControl::Observe)
            .await?
            .now,
        base
    );
    if matches!(platform, AutomationClockPlatform::Workerd) {
        pin_registry(client, base).await?;
    }
    let service = durable_steps::Uuid::now_v7().to_string();
    let peer = durable_steps::Uuid::now_v7();
    let session = durable_steps::Uuid::now_v7();
    let credential = format!("expired-project-lease-{service}");
    let mut machine = NativeBoardSession(
        enroll_admitted_peer(
            client,
            &service,
            &peer.to_string(),
            &credential,
            &session.to_string(),
        )
        .await?,
    );
    let proof = scenario(
        client,
        &mut machine,
        &service,
        &credential,
        session,
        PeerId::new(peer)?,
        platform,
    )
    .await;
    let closed = machine.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn scenario(
    client: &ConformanceClient,
    machine: &mut NativeBoardSession,
    service: &str,
    credential: &str,
    reported_session: durable_steps::Uuid,
    peer: PeerId,
    platform: AutomationClockPlatform,
) -> ConformanceResult<()> {
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let actor: RecordedActor = serde_json::from_value(serde_json::json!({
        "kind":"session","session":{
            "endpoint":{"serviceId":service,"endpointId":"codex-local"},
            "sessionId":reported_session.to_string()
        }
    }))?;
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("expired lease automation fixture".to_owned())?,
            description: Description::try_from("normal registered project".to_owned())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("lease fixture board".to_owned())?,
            description: Description::try_from("known complete project inventory".to_owned())?,
            board_kind: message_board::BoardKind::Area {},
        },
        FleetOperation::ProjectDirectoryCreate {
            project: ProjectId::try_from(database.to_string())?,
            board,
            title: ResourceName::try_from("registered lease".to_owned())?,
        },
    ] {
        let decision = machine
            .write(&FleetRequest {
                request_id: message_board::RequestId::try_from(
                    durable_steps::Uuid::now_v7().to_string(),
                )?,
                workspace: workspace.clone(),
                actor: actor.clone(),
                acting_for: None,
                protocol_version: message_board::ProtocolVersion::CURRENT,
                operation,
            })
            .await?;
        assert!(
            matches!(decision, RequestDecision::Accepted { .. }),
            "normal project metadata refused"
        );
    }
    let registration = machine
        .operation(OperationRequest::Lease(LeaseRequest::Register(
            Registration {
                attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                database,
                writer: peer,
                origin: Origin::Empty,
            },
        )))
        .await?;
    let OperationAnswer::Lease(LeaseAnswer::Register(RegistrationAnswer::Registered {
        lease, ..
    })) = registration
    else {
        return Err("normal registered project did not return its real lease".into());
    };
    assert_eq!(lease.database, database);
    assert_eq!(lease.holder, peer);
    assert_eq!(lease.epoch, Epoch::new(1)?);
    assert_eq!(lease.state, LeaseState::Held);
    let prefix = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
    let piece = Piece::<BoardAnnotation>::new(PieceContents {
        database,
        epoch: Epoch::new(1)?,
        continues_from: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)),
        through: prefix,
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitSequence::new(1),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE automation_lease_fixture(id TEXT NOT NULL PRIMARY KEY)".into(),
            ],
        }],
        part: None,
        annotation: None,
    })?;
    let applied = machine
        .operation(OperationRequest::Replication(ReplicationRequest::Piece(
            piece,
        )))
        .await?;
    assert!(
        matches!(applied, OperationAnswer::Replication(ReplicationAnswer::Applied { through }) if through == prefix),
        "normal registered Copy did not commit its real prefix"
    );
    let instruction = require_instruction(
        call(
            machine,
            AutomationCall::InstructionCreate(InstructionCreateParams {
                operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
                text: "fires after every project lease expires"
                    .to_owned()
                    .try_into()?,
            }),
        )
        .await?,
    )?;
    call(
        machine,
        AutomationCall::PullAutomationBatches(PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
        }),
    )
    .await?;
    let due = GridTime::next_after(lease.expires_at.unix_millis())?;
    let due_time = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(due.unix_millis())
        .ok_or("invalid lease fixture due")?;
    let created = call(
        machine,
        AutomationCall::ScheduleCreate(ScheduleCreateRequest {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: TimingRequest::At {
                    at: due_time
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                        .try_into()?,
                },
                enabled: true,
                destination: ExecutionDestination::OwnedThread {
                    target: serde_json::from_value(serde_json::json!({
                        "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                        "sessionId":reported_session.to_string()
                    }))?,
                    cwd: "/workspace".into(),
                },
                execution_timeout_seconds: None,
                model: None,
                effort: Some("high".into()),
            },
        }),
    )
    .await?;
    let AutomationAnswer::Schedule(schedule) = created else {
        return Err("no-project-lease Schedule refused".into());
    };
    SinkExt::close(&mut machine.0.socket).await?;
    let expired_ms = lease
        .expires_at
        .unix_millis()
        .checked_add(1)
        .ok_or("lease expiry overflow")?;
    let expired_at = RegistryTime(
        chrono::DateTime::from_timestamp_millis(expired_ms)
            .ok_or("invalid expired fixture clock")?,
    );
    match platform {
        AutomationClockPlatform::Tokio => {
            client
                .automation_clock(AutomationClockControl::Advance { now: expired_at })
                .await?;
        }
        AutomationClockPlatform::Workerd => pin_registry(client, expired_at).await?,
    }
    let (socket, databases) = reconnect(client, credential).await?;
    machine.0.socket = socket;
    let mut expected = lease.clone();
    expected.state = LeaseState::Expired;
    expected.remaining_ms = 0;
    assert_eq!(
        databases,
        vec![DatabaseStatus::Ready {
            database,
            acked: prefix,
            lease: expected
        }],
        "nonempty Welcome did not witness every known project lease expired"
    );
    println!(
        "AUTOMATION_EXPIRED_LEASE_INVENTORY {}",
        serde_json::to_string(&serde_json::json!({
            "knownDatabaseIds":[database], "registered":lease, "expiredWelcome":databases,
            "registryClock":expired_at, "automationDueMs":due.unix_millis(),
        }))?
    );
    assert!(
        pull(machine).await?.batches.is_empty(),
        "Automation fired before its due after the lease witness"
    );
    let fired = client
        .automation_clock(AutomationClockControl::Advance {
            now: RegistryTime(due_time),
        })
        .await?;
    let custody = pull(machine).await?;
    assert_eq!(custody.batches.len(), 1);
    let batch: AutomationBatch = serde_json::from_slice(
        &custody
            .batches
            .first()
            .ok_or("lease-independent Batch missing")?
            .frozen_bytes,
    )?;
    let Some(WorkEntry::Firing(entry)) = batch.entries.entries().first() else {
        return Err("lease-independent Firing missing".into());
    };
    assert_eq!(entry.automation_id.as_str(), schedule.schedule_id.as_str());
    assert_eq!(entry.due_at, due);
    client
        .automation_clock(AutomationClockControl::InvokeAlarm)
        .await?;
    assert_eq!(
        serde_json::to_value(pull(machine).await?)?,
        serde_json::to_value(&custody)?
    );
    println!(
        "AUTOMATION_EXPIRED_LEASE_FIRING {}",
        serde_json::to_string(&serde_json::json!({
            "firingId":entry.firing_id,"dueMs":entry.due_at,"callbackCount":fired.callbacks.len(),
            "batchId":batch.batch_id,"knownLeaseCount":1,
        }))?
    );
    Ok(())
}

async fn pin_registry(client: &ConformanceClient, now: RegistryTime) -> ConformanceResult<()> {
    let observed = crate::composed_client::control(
        client,
        ObjectAddress::Registry,
        RuntimeControl::SetTime { now },
    )
    .await?;
    assert!(matches!(observed.clock, ClockObservation::Pinned { now: actual } if actual == now));
    Ok(())
}
async fn reconnect(
    client: &ConformanceClient,
    credential: &str,
) -> ConformanceResult<(WebSocket, Vec<DatabaseStatus>)> {
    let mut socket = client
        .with_machine_access(
            client
                .client
                .get(format!("{}/v1/session", client.base_url))
                .bearer_auth(credential),
            credential,
        )?
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let session = SessionId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id: session,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
        .await?
        .ok_or("lease fixture ended before Welcome")??;
    let welcome: ServerFrame<BoardConsumerProtocol> = match response {
        Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
        Message::Text(text) => serde_json::from_str(&text)?,
        _ => return Err("lease fixture received a non-Welcome frame".into()),
    };
    match welcome {
        ServerFrame::Welcome {
            session_id,
            databases,
        } if session_id == session => Ok((socket, databases)),
        _ => Err("lease fixture lost exact Hello/Welcome correlation".into()),
    }
}
