use collaboration_runtime::FleetClient;
#[path = "support/native_registry.rs"]
pub mod native_registry;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RegistrationCalls(Mutex<Vec<ProjectId>>);
impl collaboration_contracts::ProjectRegistration for RegistrationCalls {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> collaboration_contracts::ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(collaboration_contracts::ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> collaboration_contracts::ProjectRegistrationFuture<'runtime> {
        Box::pin(async move {
            self.0
                .lock()
                .map_err(
                    |_| collaboration_contracts::ProjectRegistrationError::Runtime {
                        message: "registration observation poisoned".into(),
                    },
                )?
                .push(project.clone());
            Ok(collaboration_contracts::ProjectRegistrationState::Pending)
        })
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn offline_project_creation_waits_for_the_central_decision_before_registration()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440ca4".to_owned())?,
        machine: MachineLabel::try_from("offline project creator".to_owned())?,
    };
    let registration = Arc::new(RegistrationCalls::default());
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let actor = RecordedActor::Human {
        human_id: HumanId::try_from("owner".to_owned())?,
    };
    let make_request = |operation| FleetRequest {
        request_id: RequestId::generate(),
        workspace: workspace.clone(),
        actor: actor.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    };
    let (connected, runtime) =
        board_central_test_support::fleet_client(directory.path(), registration.clone(), &origin)
            .await?;
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: "offline workspace".to_owned().try_into()?,
            description: "offline project proof".to_owned().try_into()?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: "offline board".to_owned().try_into()?,
            description: "offline project proof".to_owned().try_into()?,
            board_kind: BoardKind::Area {},
        },
    ] {
        assert!(matches!(
            connected
                .submit(&make_request(operation), &origin)
                .await?
                .result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
    connected.close().await;
    runtime.shutdown().await?;

    let offered = make_request(FleetOperation::ProjectDirectoryCreate {
        project: project.clone(),
        board,
        title: "queued project".to_owned().try_into()?,
    });
    let offline = FleetClient::open(directory.path(), registration.clone(), None).await?;
    assert_eq!(
        offline.submit(&offered, &origin).await?.result,
        RequestResult::PendingUnreachable
    );
    assert_eq!(offline.pending_requests().await?.len(), 1);
    assert_eq!(
        offline
            .pending_requests()
            .await?
            .first()
            .ok_or("missing queued project")?
            .request,
        offered
    );
    assert!(offline.record(&project.clone().into()).await?.is_none());
    assert!(
        registration
            .0
            .lock()
            .map_err(|_| "registration observation poisoned")?
            .is_empty()
    );
    assert!(
        !directory
            .path()
            .join("projects")
            .join(project.as_str())
            .exists()
    );
    assert!(!directory.path().join("fleet.sqlite").exists());
    offline.close().await;

    let (reconnected, runtime) =
        board_central_test_support::fleet_client(directory.path(), registration.clone(), &origin)
            .await?;
    let reply = reconnected.submit(&offered, &origin).await?;
    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    assert!(reconnected.pending_requests().await?.is_empty());
    assert!(
        matches!(reconnected.record(&project.clone().into()).await?, Some(Record::DirectoryEntry(record)) if record.id == project)
    );
    let registrations = registration
        .0
        .lock()
        .map_err(|_| "registration observation poisoned")?
        .clone();
    assert!(!registrations.is_empty());
    assert!(registrations.iter().all(|actual| actual == &project));
    assert!(!directory.path().join("fleet.sqlite").exists());
    reconnected.close().await;
    runtime.shutdown().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn central_reply_cache_and_original_outbox_input_survive_restart()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let request = FleetRequest {
        request_id: RequestId::generate(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor::declared(Identity::Human {
            human_id: HumanId::try_from("caller".to_owned())?,
        }),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("work".to_owned())?,
            description: Description::try_from("fleet proof".to_owned())?,
        },
    };
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("fleet fixture".to_owned())?,
    };
    let offline = FleetClient::open(
        directory.path(),
        std::sync::Arc::new(native_registry::PendingRegistration),
        None,
    )
    .await?;
    assert_eq!(
        offline.submit(&request, &origin).await?.result,
        RequestResult::PendingUnreachable
    );
    offline.close().await;
    let (client, runtime) = board_central_test_support::fleet_client(
        directory.path(),
        std::sync::Arc::new(native_registry::PendingRegistration),
        &origin,
    )
    .await?;
    let first = client.submit(&request, &origin).await?;
    assert!(matches!(
        first.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let Some(Record::Workspace(workspace)) =
        client.record(&request.workspace.clone().into()).await?
    else {
        panic!("cached workspace");
    };
    assert_eq!(workspace.name.as_str(), "work");
    assert_eq!(workspace.description.as_str(), "fleet proof");
    assert_eq!(workspace.workspace_id, request.workspace);
    assert_eq!(workspace.envelope.actor, request.actor);
    assert!(
        matches!(&first.result, RequestResult::Decided(RequestDecision::Accepted { effects, .. })
        if effects.entries() == [(request.workspace.clone().into(), workspace.envelope.position.clone())])
    );
    assert!(client.pending_requests().await?.is_empty());
    let outbox =
        message_board_storage::FleetOutbox::open(&directory.path().join("fleet-outbox.sqlite"))
            .await?;
    let queued = outbox
        .request(&request.request_id)
        .await?
        .ok_or("settled request missing")?;
    assert_eq!(queued.request, request);
    assert!(matches!(
        queued.state,
        message_board_storage::FleetOutboxState::Decided { .. }
    ));
    outbox.close().await;
    client.close().await;
    runtime.shutdown().await?;
    let offline_replay = FleetClient::open(
        directory.path(),
        std::sync::Arc::new(native_registry::PendingRegistration),
        None,
    )
    .await?;
    assert_eq!(
        offline_replay.submit(&request, &origin).await?,
        first,
        "a durable decision replays without a live machine session"
    );
    assert_eq!(
        offline_replay
            .record(&request.workspace.clone().into())
            .await?,
        Some(Record::Workspace(workspace.clone()))
    );
    offline_replay.close().await;
    let (reopened, runtime) = board_central_test_support::fleet_client(
        directory.path(),
        std::sync::Arc::new(native_registry::PendingRegistration),
        &origin,
    )
    .await?;
    let replay = reopened.submit(&request, &origin).await?;
    assert_eq!(replay, first);
    assert_eq!(
        reopened.record(&request.workspace.clone().into()).await?,
        Some(Record::Workspace(workspace))
    );
    assert!(reopened.pending_requests().await?.is_empty());
    let mut conflict = request.clone();
    conflict.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("different".to_owned())?,
        description: Description::try_from("fleet proof".to_owned())?,
    };
    assert_eq!(
        reopened.submit(&conflict, &origin).await?.result,
        RequestResult::ConflictingRequest
    );
    assert!(!directory.path().join("fleet.sqlite").exists());
    reopened.close().await;
    runtime.shutdown().await?;
    Ok(())
}
