use collaboration_service::{
    FleetClient, LocalLeaseAuthority, MachineIdentity, ProjectAdmission, ProjectAdmissionProps,
    ProjectStoreRegistry,
};
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;
use std::sync::Arc;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn public_creation_bootstraps_without_authority_and_replays_without_another_feed_write()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let fleet = Arc::new(FleetClient::open_local_stand_in(directory.path()).await?);
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let actor = Identity::Human {
        human_id: HumanId::try_from("creator".to_owned())?,
    };
    let recorded = RecordedActor {
        actor: ActorKind::Declared(actor.clone()),
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
        machine: MachineLabel::try_from("test".to_owned())?,
    };
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("workspace".to_owned())?,
            description: Description::try_from("".to_owned())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("board".to_owned())?,
            description: Description::try_from("".to_owned())?,
            board_kind: BoardKind::Area {},
        },
    ] {
        let result = fleet
            .submit(
                &FleetRequest {
                    request_id: RequestId::generate(),
                    workspace: workspace.clone(),
                    actor: recorded.clone(),
                    acting_for: None,
                    resubmits: None,
                    protocol_version: ProtocolVersion::CURRENT,
                    operation,
                },
                &FleetIds {
                    generated_record: RecordId::generate(),
                },
            )
            .await?;
        assert!(matches!(
            result.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
    let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
    let admission = ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        leases: Arc::new(LocalLeaseAuthority::default()),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            )?,
            Some("test"),
        )?,
    })?;
    let request = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: None,
        project_id: project.clone(),
        board_id: board,
        title: ResourceName::try_from("project".to_owned())?,
        actor,
        acting_for: None,
    };
    let first = admission.create_project(&request).await?;
    assert!(matches!(
        first.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("created file missing")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("created state missing".into());
    };
    assert_eq!(state.status, None);
    assert_eq!(state.pinned_definition, None);
    assert!(read.records(RecordKind::Workstream).await?.is_empty());
    assert!(read.records(RecordKind::RoleAssignment).await?.is_empty());
    let high_water = read.high_water();
    read.finish().await?;
    assert_eq!(
        admission.create_project(&request).await?.result,
        first.result
    );
    assert_eq!(store.snapshot().await?.high_water(), high_water);
    assert!(fleet.record(&project.into()).await?.is_some());
    Ok(())
}

#[path = "support/board_fixture.rs"]
pub mod board_fixture;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn directory_name_rejection_voids_creation_and_preserves_orphan_ledger()
-> Result<(), board_fixture::TestError> {
    let fixture = board_fixture::BoardFixture::start().await?;
    let actor = serde_json::from_value(fixture.actor.clone())?;
    let request = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: None,
        project_id: ProjectId::generate(),
        board_id: fixture.board.clone(),
        title: ResourceName::try_from("same title".to_owned())?,
        actor,
        acting_for: None,
    };
    let first = fixture._admission.create_project(&request).await?;
    assert!(matches!(
        first.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let collision = ProjectCreateRequest {
        request_id: RequestId::generate(),
        project_id: ProjectId::generate(),
        ..request
    };
    let result = fixture._admission.create_project(&collision).await?;
    assert!(
        matches!(&result.result,RequestResult::Decided(RequestDecision::Voided {cause:VoidCause::ProjectRejected {reason}}) if matches!(reason.as_ref(),Refusal::NameConflict {..}))
    );
    assert!(
        fixture
            ._registry
            .existing_project(&collision.project_id)
            .await?
            .is_none()
    );
    assert_eq!(
        fixture._admission.create_project(&collision).await?.result,
        result.result
    );
    fixture.stop().await
}
