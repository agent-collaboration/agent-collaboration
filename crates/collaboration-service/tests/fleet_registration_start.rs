#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! A fresh central decision starts registration within the request's settlement.
use collaboration_contracts::ProjectRegistration;
use collaboration_contracts::ProjectRegistrationFuture;
use collaboration_contracts::ProjectRegistrationState;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use std::sync::{Arc, Mutex};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Default)]
struct RegistrationStarts(Mutex<Vec<ProjectId>>);
impl ProjectRegistration for RegistrationStarts {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }

    #[allow(clippy::expect_used)]
    fn start<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("registration observations")
                .push(project.clone());
            Ok(ProjectRegistrationState::Pending)
        })
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn, clippy::expect_used)]
async fn a_newly_decided_directory_starts_registration_without_another_recovery_pass() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let starts = Arc::new(RegistrationStarts::default());
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("registration settlement proof".to_owned())?,
    };
    let (fleet, runtime) =
        board_central_test_support::fleet_client(directory.path(), starts.clone(), &origin).await?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let actor = Identity::Human {
        human_id: HumanId::try_from("directory owner".to_owned())?,
    };
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("registration workspace".to_owned())?,
            description: Description::try_from(String::new())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("registration board".to_owned())?,
            description: Description::try_from(String::new())?,
            board_kind: BoardKind::Area {},
        },
    ] {
        let reply = fleet
            .submit(
                &FleetRequest {
                    request_id: RequestId::generate(),
                    workspace: workspace.clone(),
                    actor: RecordedActor::declared(actor.clone()),
                    acting_for: None,
                    protocol_version: ProtocolVersion::CURRENT,
                    operation,
                },
                &origin,
            )
            .await?;
        assert!(matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
    assert!(
        starts
            .0
            .lock()
            .expect("registration observations")
            .is_empty()
    );
    let input = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id: board.clone(),
        title: ResourceName::try_from("new central decision".to_owned())?,
        description: None,
        actor: actor.clone(),
        acting_for: None,
    };
    let request = FleetRequest {
        request_id: input.request_id.clone(),
        workspace,
        actor: RecordedActor::declared(actor),
        acting_for: None,
        protocol_version: input.protocol_version,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: input.project_id.clone(),
            board,
            title: input.title.clone(),
        },
    };

    // The registration port is observed, while Access, central admission,
    // the machine session and durable Router settlement stay real.
    let reply = fleet.submit_creation(&input, &request, &origin).await?;

    assert!(matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    assert_eq!(
        *starts.0.lock().expect("registration observations"),
        vec![input.project_id.clone()]
    );
    assert_eq!(
        fleet.submit_creation(&input, &request, &origin).await?,
        reply
    );
    assert_eq!(
        *starts.0.lock().expect("registration observations"),
        vec![input.project_id]
    );
    runtime.shutdown().await?;
    fleet.close().await;
    Ok(())
}
