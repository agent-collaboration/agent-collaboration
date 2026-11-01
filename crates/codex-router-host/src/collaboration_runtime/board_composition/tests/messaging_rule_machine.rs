//! A successor uses the existing Host composition and a real independent enrollment.
use super::super::super::forwarded_notification_fixture::{HolderNotifications, MailWorkerStart};
use super::*;
use board_central_test_support::EnrolledTestMachine;

pub(in crate::collaboration_runtime::board_composition::tests::messaging_rule_cases) struct RuleMachine
{
    pub registry: Arc<ProjectStoreRegistry>,
    pub projects: Arc<project_writers::ProjectWriters>,
    pub admission: Arc<ProjectAdmission>,
    pub notifications: HolderNotifications,
    running: RunningMachineSession,
    machine: EnrolledTestMachine,
}
impl RuleMachine {
    pub async fn open(
        holder: &claim_fixture::ClaimFixture,
        origin: &ActorOrigin,
        worker_start: MailWorkerStart,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let machine = holder
            .central
            .additional_machine(
                origin,
                fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000051082".to_owned(),
                )?,
            )
            .await?;
        let root = holder.directory.path().join("rule-successor");
        let identity = MachineIdentity::new(
            UuidIdentity::try_from(origin.installation.as_str().to_owned())?,
            Some(origin.machine.as_str()),
        )?;
        let mut board = open_board_admission(
            &root,
            identity.clone(),
            Some(machine.session_inputs()?),
            None,
        )
        .await?;
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
