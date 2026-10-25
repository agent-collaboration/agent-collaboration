//! Each case owns a real central, normal machine session and unpublished registry.
use super::*;
use board_central_test_support::TestCentral;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use collaboration_service::ProjectClaim;

pub(super) struct ClaimFixture {
    pub directory: tempfile::TempDir,
    pub central: Arc<TestCentral>,
    pub projects: Arc<project_writers::ProjectWriters>,
    pub registry: Arc<ProjectStoreRegistry>,
    pub admission: Arc<ProjectAdmission>,
    pub fleet: Weak<collaboration_service::FleetClient>,
    #[cfg(feature = "runtime-test-support")]
    pub notifications: Option<super::forwarded_notification_fixture::HolderNotifications>,
    running: Option<super::super::machine_session::RunningMachineSession>,
}

impl ClaimFixture {
    pub async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::start_configured(false).await
    }

    #[cfg(feature = "runtime-test-support")]
    pub async fn start_with_notifications() -> TestResultFixture {
        Self::start_configured(true).await
    }

    async fn start_configured(with_notifications: bool) -> TestResultFixture {
        let directory = tempfile::tempdir()?;
        let central =
            Arc::new(TestCentral::start(&directory.path().join("central"), &origin()?).await?);
        #[allow(unused_mut)] // Notification tests attach existing mail owners before activation.
        let mut board = open_board_admission(
            &directory.path().join("router"),
            identity()?,
            Some(central.session_inputs()?),
        )
        .await?;
        let session = board.session.as_ref().ok_or("session")?;
        let projects = session.projects.clone();
        let welcomed = session.welcomed.subscribe();
        let registry = projects.registry()?;
        let admission = board.admission.clone();
        let fleet = board.fleet.clone();
        #[cfg(feature = "runtime-test-support")]
        let notifications = if with_notifications {
            Some(
                super::forwarded_notification_fixture::HolderNotifications::prepare(
                    &mut board,
                    &directory.path().join("router"),
                    identity()?,
                )
                .await?,
            )
        } else {
            None
        };
        #[cfg(not(feature = "runtime-test-support"))]
        debug_assert!(!with_notifications);
        let running = board.start().ok_or("session not started")?;
        wait_welcome(welcomed).await?;
        Ok(Self {
            directory,
            central,
            projects,
            registry,
            admission,
            fleet,
            #[cfg(feature = "runtime-test-support")]
            notifications,
            running: Some(running),
        })
    }

    pub async fn claim(
        &self,
        project: &ProjectId,
    ) -> Result<ProjectClaimReply, Box<dyn std::error::Error + Send + Sync>> {
        let request = ProjectClaimRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: project.clone(),
        };
        timeout(BOUND, async {
            loop {
                let reply = self.projects.claim(&request).await?;
                if !matches!(reply, ProjectClaimReply::Pending { .. }) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(reply);
                }
            }
        })
        .await?
    }

    pub async fn disconnect_holder(&mut self) -> TestResult {
        if let Some(running) = self.running.take() {
            running.shutdown().await?;
        }
        Ok(())
    }

    pub async fn shutdown(mut self) -> TestResult {
        let stopped = self.disconnect_holder().await;
        #[cfg(feature = "runtime-test-support")]
        let notifications_closed = match self.notifications.take() {
            Some(notifications) => notifications.close().await,
            None => Ok(()),
        };
        drop(self.projects);
        drop(self.registry);
        let closed = Arc::try_unwrap(self.admission)
            .map_err(|_| io::Error::other("admission still owned"))?
            .close()
            .await;
        let central_closed = Arc::try_unwrap(self.central)
            .map_err(|_| io::Error::other("central still owned"))?
            .shutdown()
            .await;
        stopped?;
        #[cfg(feature = "runtime-test-support")]
        notifications_closed?;
        closed?;
        central_closed?;
        Ok(())
    }
}

type TestResultFixture = Result<ClaimFixture, Box<dyn std::error::Error + Send + Sync>>;
