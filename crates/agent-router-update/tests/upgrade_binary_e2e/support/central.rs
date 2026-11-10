use super::{E2eResult, promote_error, promote_result};
use crate::central_fixture::RestartableCentral;
use std::path::Path;

pub struct CentralHarness(RestartableCentral);

impl CentralHarness {
    pub async fn start() -> E2eResult<Self> {
        promote_result(RestartableCentral::start().await).map(Self)
    }

    pub fn root(&self) -> &Path {
        self.0.root()
    }

    pub fn base_url(&self) -> E2eResult<&str> {
        self.0.base_url().map_err(promote_error)
    }

    pub async fn stop_owner(&mut self) -> E2eResult<()> {
        promote_result(self.0.stop_owner().await)
    }

    pub async fn reopen_owner(&mut self) -> E2eResult<String> {
        promote_result(self.0.reopen_owner().await)
    }

    pub async fn shutdown(self) -> E2eResult<()> {
        promote_result(self.0.shutdown().await)
    }
}
