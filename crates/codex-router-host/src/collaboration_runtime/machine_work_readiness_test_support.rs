//! The integration fixture uses the existing session lifecycle and one real capability pull.
use super::CollaborationRuntime;
use std::io;
impl CollaborationRuntime {
    /// Test-only causal readiness: wait for Welcome, then advertise machine-work V1 once.
    pub async fn runtime_test_causal_machine_work_readiness_pull(&self) -> io::Result<()> {
        self.board_session
            .as_ref()
            .ok_or_else(|| io::Error::other("test machine session is unavailable"))?
            .causal_machine_work_readiness_pull_for_test()
            .await
    }
}
