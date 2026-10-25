//! The central and private roots have one owner and one finalizer per producer scenario.
use super::{TestResult, fleet_host};
use board_central_test_support::EnrollmentTestCentral;
use std::{io, path::PathBuf};

pub struct ProducerFixture {
    pub root: tempfile::TempDir,
    pub router_root: PathBuf,
    pub central: EnrollmentTestCentral,
}
impl ProducerFixture {
    pub async fn start() -> TestResult<Self> {
        let root = fleet_host::fixture_root()?;
        let router_root = root.path().join("router");
        let central = EnrollmentTestCentral::start(&root.path().join("central"))
            .await
            .map_err(io::Error::other)?;
        Ok(Self {
            root,
            router_root,
            central,
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
                    .finish(Ok(()))
                    .await
                    .map_err(|error| io::Error::other(error.to_string()).into())
            },
            || Ok(()),
        )
        .await
        .map_err(io::Error::other)
        .map_err(Into::into)
    }
    async fn finish(self, observed: TestResult) -> TestResult {
        let stopped = self.central.shutdown().await.map_err(io::Error::other);
        observed?;
        stopped?;
        Ok(())
    }
}
