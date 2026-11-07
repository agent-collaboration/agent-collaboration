//! A successor uses the existing Host composition and a real independent enrollment.
use super::super::super::forwarded_notification_fixture::{HolderNotifications, MailWorkerStart};
use super::*;
use board_central_test_support::EnrolledTestMachine;

#[path = "messaging_recipient_observation.rs"]
mod recipient_observation;

pub(in crate::collaboration_runtime::board_composition::tests::messaging_rule_cases) struct RuleMachine
{
    pub peer: durable_steps::Uuid,
    pub registry: Arc<ProjectStoreRegistry>,
    pub projects: Arc<project_writers::ProjectWriters>,
    pub admission: Arc<ProjectAdmission>,
    pub notifications: HolderNotifications,
    running: RunningMachineSession,
    machine: EnrolledTestMachine,
}
impl RuleMachine {
    pub fn observe_mail_wire(
        &self,
    ) -> Result<
        board_central_test_support::MailWireObservation,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.machine.observe_mail_wire(None)
    }
    pub async fn open(
        holder: &claim_fixture::ClaimFixture,
        origin: &ActorOrigin,
        worker_start: MailWorkerStart,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::open_at(
            holder,
            origin,
            worker_start,
            "rule-successor",
            "019f0000-0000-7000-8000-000000051082",
        )
        .await
    }

    pub async fn open_recipient(
        holder: &claim_fixture::ClaimFixture,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let origin = ActorOrigin {
            installation: super::RECIPIENT.to_owned().try_into()?,
            machine: "rule recipient".to_owned().try_into()?,
        };
        Self::open_at(
            holder,
            &origin,
            MailWorkerStart::Deferred,
            "rule-recipient",
            "019f0000-0000-7000-8000-000000051083",
        )
        .await
    }

    async fn open_at(
        holder: &claim_fixture::ClaimFixture,
        origin: &ActorOrigin,
        worker_start: MailWorkerStart,
        directory_name: &str,
        peer: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let peer = fleet_protocol::PeerId::try_from(peer.to_owned())?;
        let machine = holder.central.additional_machine(origin, peer).await?;
        let root = holder.directory.path().join(directory_name);
        let identity = MachineIdentity::new(
            UuidIdentity::try_from(origin.installation.as_str().to_owned())?,
            Some(origin.machine.as_str()),
        )?;
        let mut board =
            open_board_admission(&root, identity.clone(), Some(machine.session_inputs()?)).await?;
        let session = board.session.as_ref().ok_or("successor session")?;
        let projects = session.projects.clone();
        let registry = projects.registry()?;
        let admission = board.admission.clone();
        let notifications =
            HolderNotifications::prepare_with_worker(&mut board, &root, identity, worker_start)
                .await?;
        let running = board.start().ok_or("successor running session")?;
        wait_welcome(projects.welcomed.clone()).await?;
        Ok(Self {
            peer: peer.as_uuid(),
            registry,
            projects,
            admission,
            notifications,
            running,
            machine,
        })
    }
    pub async fn close(self) -> TestResult {
        self.running.shutdown().await?;
        self.notifications.close().await?;
        drop(self.projects);
        drop(self.registry);
        Arc::try_unwrap(self.admission)
            .map_err(|_| "successor admission retained")?
            .close()
            .await?;
        self.machine.shutdown().await?;
        Ok(())
    }
}
