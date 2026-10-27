//! Native outbox/port proof; the recording port stands in for A3c's Host runtime.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
use collaboration_service::{
    ProjectRegistration, ProjectRegistrationError, ProjectRegistrationFuture,
    ProjectRegistrationState,
};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{FleetOutbox, FleetOutboxState};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct RecordingRegistration {
    outbox: FleetOutbox,
    requests: BTreeMap<ProjectId, RequestId>,
    attempted: Mutex<Vec<ProjectId>>,
    interrupt_once: AtomicBool,
}
impl ProjectRegistration for RecordingRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async move {
            let failed = |message: &str| ProjectRegistrationError::Runtime {
                message: message.to_owned(),
            };
            let request = self
                .requests
                .get(project)
                .ok_or_else(|| failed("unexpected project"))?;
            let queued = self
                .outbox
                .request(request)
                .await
                .map_err(|_| failed("native outbox read"))?
                .ok_or_else(|| failed("directory request missing"))?;
            if !matches!(
                queued.state,
                FleetOutboxState::Decided {
                    decision: RequestDecision::Accepted { .. }
                }
            ) {
                return Err(failed(
                    "registration started without a durable Accepted fleet decision",
                ));
            }
            self.attempted.lock().await.push(project.clone());
            if self.interrupt_once.swap(false, Ordering::SeqCst) {
                return Err(failed("fixture interruption after the fleet answer"));
            }
            Ok(ProjectRegistrationState::Pending)
        })
    }
}

#[tokio::test]
async fn settled_directory_requests_are_durable_before_registration_and_recovered_after_reopen()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let accepted_project = ProjectId::generate();
    let rejected_project = ProjectId::generate();
    let accepted_id = RequestId::generate();
    let rejected_id = RequestId::generate();
    let requests = BTreeMap::from([
        (accepted_project.clone(), accepted_id.clone()),
        (rejected_project.clone(), rejected_id.clone()),
    ]);
    let origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("registration fixture".to_owned())?,
    };
    let port = Arc::new(RecordingRegistration {
        outbox: FleetOutbox::open(&directory.path().join("fleet-outbox.sqlite")).await?,
        requests: requests.clone(),
        attempted: Mutex::new(Vec::new()),
        interrupt_once: AtomicBool::new(true),
    });
    let (client, runtime) =
        board_central_test_support::fleet_client(directory.path(), port.clone(), &origin).await?;
    let actor = RecordedActor::declared(Identity::Human {
        human_id: HumanId::try_from("owner".to_owned())?,
    });
    let request = |request_id, operation| FleetRequest {
        request_id,
        workspace: workspace.clone(),
        actor: actor.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    };
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("workspace".to_owned())?,
            description: Description::try_from(String::new())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("board".to_owned())?,
            description: Description::try_from(String::new())?,
            board_kind: BoardKind::Area {},
        },
    ] {
        assert!(matches!(
            client
                .submit(&request(RequestId::generate(), operation), &origin)
                .await?
                .result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
    // The real central answer is settled before the recording port interrupts.
    assert!(matches!(
        client
            .submit(
                &request(
                    accepted_id.clone(),
                    FleetOperation::ProjectDirectoryCreate {
                        project: accepted_project.clone(),
                        board,
                        title: ResourceName::try_from("accepted".to_owned())?,
                    }
                ),
                &origin
            )
            .await,
        Err(collaboration_service::FleetClientError::Registration(_))
    ));
    let accepted = port
        .outbox
        .request(&accepted_id)
        .await?
        .ok_or("accepted outbox missing")?;
    let FleetOutboxState::Decided {
        decision: accepted_decision @ RequestDecision::Accepted { .. },
    } = accepted.state
    else {
        return Err("registration interrupted before central acceptance was durable".into());
    };
    let rejected = client
        .submit(
            &request(
                rejected_id.clone(),
                FleetOperation::ProjectDirectoryCreate {
                    project: rejected_project.clone(),
                    board: BoardId::generate(),
                    title: ResourceName::try_from("rejected".to_owned())?,
                },
            ),
            &origin,
        )
        .await?;
    let RequestResult::Decided(rejected_decision @ RequestDecision::Rejected { .. }) =
        rejected.result
    else {
        return Err("invalid directory was not rejected by the real core".into());
    };
    assert_eq!(*port.attempted.lock().await, vec![accepted_project.clone()]);
    assert!(
        matches!(port.outbox.request(&rejected_id).await?.ok_or("rejected outbox")?.state,
        FleetOutboxState::Decided { decision } if decision == rejected_decision)
    );
    client.close().await;
    runtime.shutdown().await?;
    Arc::try_unwrap(port)
        .map_err(|_| "registration port still owned")?
        .outbox
        .close()
        .await;

    let port = Arc::new(RecordingRegistration {
        outbox: FleetOutbox::open(&directory.path().join("fleet-outbox.sqlite")).await?,
        requests,
        attempted: Mutex::new(Vec::new()),
        interrupt_once: AtomicBool::new(false),
    });
    // The fixture's real Welcome then calls FleetClient's synchronization owner.
    let (reopened, runtime) =
        board_central_test_support::fleet_client(directory.path(), port.clone(), &origin).await?;
    assert_eq!(
        *port.attempted.lock().await,
        vec![accepted_project],
        "recovery must retry only Accepted directory decisions"
    );
    assert!(
        matches!(port.outbox.request(&accepted_id).await?.ok_or("reopened accepted outbox")?.state,
        FleetOutboxState::Decided { decision } if decision == accepted_decision)
    );
    assert!(reopened.pending_requests().await?.is_empty());
    reopened.close().await;
    runtime.shutdown().await?;
    Arc::try_unwrap(port)
        .map_err(|_| "registration port still owned")?
        .outbox
        .close()
        .await;
    Ok(())
}
