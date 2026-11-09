//! Compose native project storage with an explicit enrollment input from the Host.
use board_fleet_sync::BoardSessionInputs;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::ProjectRegistration;
use collaboration_contracts::ProjectRegistrationError;
use collaboration_contracts::ProjectRegistrationFuture;
use collaboration_contracts::ProjectReplicationPosition;
use collaboration_runtime::FleetClient;
use collaboration_runtime::MailDeliverySession;
use collaboration_runtime::ProjectStoreRegistry;
use collaboration_service::MailDeliveryRuntime;
use collaboration_service::NotificationPublisher;
use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;
use machine_session::PreparedMachineSession;
use message_board::ProjectId;
use runtime_primitives::SystemClock;
use std::{
    io,
    path::Path,
    sync::{Arc, Weak},
};

#[path = "board_composition/archive_observation.rs"]
mod archive_observation;
#[path = "board_composition/central_read_copy.rs"]
mod central_read_copy;
#[cfg(test)]
#[path = "board_composition/tests/fleet_service.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod fleet_service;
#[path = "board_composition/install_attempts.rs"]
mod install_attempts;
#[path = "board_composition/machine_link.rs"]
mod machine_link;
#[path = "board_composition/machine_notice_reactions.rs"]
mod machine_notice_reactions;
#[path = "board_composition/machine_session.rs"]
mod machine_session;
#[path = "board_composition/notice_receipts.rs"]
mod notice_receipts;
#[path = "board_composition/project_claims.rs"]
mod project_claims;
#[path = "board_composition/project_installation.rs"]
mod project_installation;
#[path = "board_composition/project_writer.rs"]
mod project_writer;
#[path = "board_composition/project_writers.rs"]
mod project_writers;
#[path = "board_composition/received_grants.rs"]
mod received_grants;
#[path = "board_composition/server_calls.rs"]
mod server_calls;
#[path = "board_composition/snapshot_installation.rs"]
mod snapshot_installation;
#[cfg(test)]
#[path = "board_composition/tests.rs"]
mod tests;

pub(super) use machine_session::RunningMachineSession;

pub(super) struct BoardComposition {
    link: board_fleet_sync::FleetLinkObservation,
    link_tasks: tokio::task::JoinSet<io::Result<()>>,
    pub admission: Arc<ProjectAdmission>,
    pub replication: Option<Arc<dyn ProjectReplicationPosition>>,
    pub claim: Option<Arc<dyn collaboration_contracts::ProjectClaim>>,
    session: Option<PreparedMachineSession>,
    fleet: Weak<FleetClient>,
    mail: Weak<MailDeliveryRuntime>,
    publisher: Weak<NotificationPublisher>,
}

impl BoardComposition {
    pub fn link_observation(&self) -> board_fleet_sync::FleetLinkObservation {
        self.link.clone()
    }
    pub fn fleet_client(&self) -> Option<Arc<FleetClient>> {
        self.fleet.upgrade()
    }

    pub fn mail_session(&self) -> Option<MailDeliverySession> {
        self.session.as_ref().map(|session| MailDeliverySession {
            requests: session.requests.clone(),
            welcomed: session.welcomed.subscribe(),
            peer: session.peer,
            relay: session.relay,
        })
    }

    pub fn attach_mail(&mut self, mail: &Arc<MailDeliveryRuntime>) {
        self.mail = Arc::downgrade(mail);
    }

    pub fn attach_publisher(&mut self, publisher: &Arc<NotificationPublisher>) {
        self.publisher = Arc::downgrade(publisher);
    }

    pub fn start(self) -> Option<RunningMachineSession> {
        self.session.map(|session| {
            session.start(
                Arc::downgrade(&self.admission),
                self.fleet,
                self.mail,
                self.publisher,
                self.link_tasks,
            )
        })
    }
}

// No session is constructed for the production None input. Directory decisions
// cannot turn this absent runtime into a fictitious pending or ready operation.
struct UnattachedProjectRegistration;
impl ProjectRegistration for UnattachedProjectRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        self.start(project)
    }

    fn start<'runtime>(
        &'runtime self,
        _: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async {
            Err(ProjectRegistrationError::Runtime {
                message: "the machine session is not attached".to_owned(),
            })
        })
    }
}

#[cfg(test)]
pub(super) async fn open_board_admission(
    directory: &Path,
    identity: MachineIdentity,
    session_inputs: Option<BoardSessionInputs>,
) -> io::Result<BoardComposition> {
    crate::enrollment_reset::resume_enrollment_reset(directory, directory).await?;
    Box::pin(open_board_admission_with_enrollment(
        directory,
        identity,
        session_inputs,
        directory,
    ))
    .await
}

#[cfg(test)]
pub(super) async fn open_board_admission_with_enrollment(
    directory: &Path,
    identity: MachineIdentity,
    session_inputs: Option<BoardSessionInputs>,
    enrollment_directory: &Path,
) -> io::Result<BoardComposition> {
    Box::pin(open_observed_board_admission(
        directory,
        identity,
        session_inputs,
        enrollment_directory,
    ))
    .await
    .0
}

pub(super) async fn open_observed_board_admission(
    directory: &Path,
    identity: MachineIdentity,
    session_inputs: Option<BoardSessionInputs>,
    enrollment_directory: &Path,
) -> (
    io::Result<BoardComposition>,
    board_fleet_sync::FleetLinkObservation,
) {
    use board_fleet_sync::FleetLinkObservation;
    let link = match &session_inputs {
        Some(inputs) => match machine_link::PreparedMachineLink::prepare(directory, inputs).await {
            Ok(link) => Some(link),
            Err(error) => {
                tracing::error!(event = "fleet.link.preparationFailed", %error, "FleetCache link observation unavailable");
                let observation = FleetLinkObservation::Unavailable {
                    message: error.to_string(),
                };
                return (Err(error), observation);
            }
        },
        None => None,
    };
    let result = prepare_board_admission(
        directory,
        identity,
        session_inputs,
        enrollment_directory,
        link.as_ref(),
    )
    .await;
    match result {
        Ok(mut board) => {
            if let Some(link) = link {
                board.link = FleetLinkObservation::Live(link.reader);
                board.link_tasks = link.tasks;
            }
            let observation = board.link_observation();
            (Ok(board), observation)
        }
        Err(error) => {
            tracing::error!(event = "fleet.link.preparationFailed", %error, "BoardComposition preparation failed");
            let observation = match link {
                Some(link) => link.failed(&error).await,
                None => FleetLinkObservation::Unavailable {
                    message: error.to_string(),
                },
            };
            (Err(error), observation)
        }
    }
}

async fn prepare_board_admission(
    directory: &Path,
    identity: MachineIdentity,
    session_inputs: Option<BoardSessionInputs>,
    enrollment_directory: &Path,
    link: Option<&machine_link::PreparedMachineLink>,
) -> io::Result<BoardComposition> {
    let marker = enrollment_directory.to_owned();
    if tokio::task::spawn_blocking(move || {
        crate::enrollment_reset::reset_pending_in_directory(&marker)
    })
    .await
    .map_err(io::Error::other)??
    {
        return Err(io::Error::other(crate::ENROLLMENT_RESTART_INSTRUCTION));
    }
    let registry = Arc::new(ProjectStoreRegistry::new(
        directory,
        session_inputs.as_ref().map(|inputs| inputs.peer),
        Arc::new(SystemClock),
    ));
    let session = match session_inputs {
        Some(inputs) => {
            // Keep nested native startup out of every Host caller's future layout.
            Some(
                Box::pin(PreparedMachineSession::prepare(
                    directory,
                    inputs,
                    &registry,
                    enrollment_directory,
                    link.ok_or_else(|| io::Error::other("configured link observation is missing"))?,
                ))
                .await?,
            )
        }
        None => None,
    };
    let registration: Arc<dyn ProjectRegistration> = match &session {
        Some(session) => session.projects.clone(),
        None => Arc::new(UnattachedProjectRegistration),
    };
    let replication = session
        .as_ref()
        .map(|session| -> Arc<dyn ProjectReplicationPosition> { session.projects.clone() });
    let fleet = Arc::new(
        FleetClient::open(
            directory,
            registration,
            session.as_ref().map(|session| session.requests.clone()),
        )
        .await
        .map_err(io::Error::other)?,
    );
    let admission = ProjectAdmission::new(ProjectAdmissionProps {
        registry,
        fleet: fleet.clone(),
        identity,
    })
    .map_err(io::Error::other)?;
    admission
        .recover_creating_projects()
        .await
        .map_err(io::Error::other)?;
    let claim = session.as_ref().map(
        |session| -> Arc<dyn collaboration_contracts::ProjectClaim> { session.projects.clone() },
    );
    Ok(BoardComposition {
        link: board_fleet_sync::FleetLinkObservation::Unconfigured,
        link_tasks: tokio::task::JoinSet::new(),
        claim,
        admission: Arc::new(admission),
        replication,
        session,
        fleet: Arc::downgrade(&fleet),
        mail: Weak::new(),
        publisher: Weak::new(),
    })
}
