//! One owner for the production Host, central and health fixture lifetime.
use super::*;

pub(super) enum EnrollmentLocation {
    RouterRoot,
    CollaborationDirectory,
}

pub(super) struct ProductionEnrollmentFixture {
    pub root: tempfile::TempDir,
    pub directory: PathBuf,
    pub peer: fleet_sync_protocol::PeerId,
    pub central: TestCentral,
    pub host: OwnedHost,
    router: PersistentRouterHealthFixture,
}
impl ProductionEnrollmentFixture {
    pub async fn start(location: EnrollmentLocation) -> TestResult<Self> {
        let root = fixture_root()?;
        let directory = root.path().join("collaboration");
        let peer = collaboration_service::load_installation_peer(&directory)?;
        let central = TestCentral::start_with_peer(
            &root.path().join("central"),
            &fixture_origin(&directory)?,
            peer,
        )
        .await
        .map_err(io::Error::other)?;
        let enrollment_root = match location {
            EnrollmentLocation::RouterRoot => root.path().join("router"),
            EnrollmentLocation::CollaborationDirectory => directory.clone(),
        };
        if let Err(error) = write_enrollment(&enrollment_root, &central) {
            let _stopped = central.shutdown().await;
            return Err(error);
        }
        let router = match PersistentRouterHealthFixture::start().await {
            Ok(router) => router,
            Err(error) => {
                let _stopped = central.shutdown().await;
                return Err(error.into());
            }
        };
        let host = match OwnedHost::spawn(root.path(), router.address()) {
            Ok(host) => host,
            Err(error) => {
                let _router_stopped = router.finish().await;
                let _central_stopped = central.shutdown().await;
                return Err(error);
            }
        };
        Ok(Self {
            root,
            directory,
            peer,
            central,
            host,
            router,
        })
    }

    pub async fn run<Observe>(self, observe: Observe) -> TestResult
    where
        Observe: for<'fixture> std::ops::AsyncFnOnce(&'fixture Self) -> TestResult,
    {
        codex_router_test_support::scenario_finalization::run_scenario(
            self,
            async |fixture: &mut Self| {
                observe(fixture)
                    .await
                    .map_err(|error| io::Error::other(error.to_string()).into())
            },
            async |fixture| {
                fixture
                    .finalize(Ok(()))
                    .await
                    .map_err(|error| io::Error::other(error.to_string()).into())
            },
            || Ok(()),
        )
        .await
        .map_err(io::Error::other)
        .map_err(Into::into)
    }
    async fn finalize(mut self, observed: TestResult) -> TestResult {
        // Attempt every owned cleanup before returning an observation or cleanup error.
        let stopped = self.host.stop().await;
        let router_stopped = self.router.finish().await;
        let central_stopped = self.central.shutdown().await.map_err(io::Error::other);
        let children_gone = assert_owned_children_gone(self.root.path()).await;
        observed?;
        require(stopped?.success(), "Host shutdown failed")?;
        router_stopped?;
        central_stopped?;
        children_gone
    }
}
