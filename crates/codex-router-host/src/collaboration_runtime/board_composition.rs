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
    pub admission: Arc<ProjectAdmission>,
    pub replication: Option<Arc<dyn ProjectReplicationPosition>>,
    pub claim: Option<Arc<dyn collaboration_contracts::ProjectClaim>>,
    session: Option<PreparedMachineSession>,
    fleet: Weak<FleetClient>,
    mail: Weak<MailDeliveryRuntime>,
    publisher: Weak<NotificationPublisher>,
}

impl BoardComposition {
    pub async fn attach_automations(
        &mut self,
        store: Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
        clock: Arc<dyn collaboration_runtime::CentralAutomationClock>,
        service_id: collaboration_protocol::UuidIdentity,
    ) -> Option<(
        Arc<collaboration_runtime::CentralAutomationReceiver>,
        collaboration_runtime::CentralReportWorker,
    )> {
        let session = self.session.as_mut()?;
        let report_clock = clock.clone();
        store
            .lock()
            .await
            .set_central_report_clock(Arc::new(move || report_clock.now_ms()));
        let transport = Arc::new(collaboration_runtime::SessionAutomationTransport::new(
            session.requests.clone(),
        ));
        let receiver = Arc::new(collaboration_runtime::CentralAutomationReceiver::new(
            store.clone(),
            transport.clone(),
            clock.clone(),
            service_id,
        ));
        session.notices.attach_automations(receiver.clone());
        Some((
            receiver,
            collaboration_runtime::CentralReportWorker::new(store, transport, clock),
        ))
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

pub(super) async fn open_board_admission(
    directory: &Path,
    identity: MachineIdentity,
    session_inputs: Option<BoardSessionInputs>,
) -> io::Result<BoardComposition> {
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
                    directory, inputs, &registry,
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
        claim,
        admission: Arc::new(admission),
        replication,
        session,
        fleet: Arc::downgrade(&fleet),
        mail: Weak::new(),
        publisher: Weak::new(),
    })
}
