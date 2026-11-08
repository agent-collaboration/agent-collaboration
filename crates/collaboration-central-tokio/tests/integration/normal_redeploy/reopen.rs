//! The closed-state carrier requires a real socket end before reopening the owner.
use super::*;

impl NormalRedeployState {
    pub(in super::super) async fn observe_server_close(
        mut self,
    ) -> ConformanceResult<ClosedNormalRedeployState> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match self.original_target.next().await {
                    None | Some(Ok(Message::Close { .. })) | Some(Err(_)) => {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    Some(Ok(_)) => {}
                }
            }
        })
        .await??;
        println!(
            "NORMAL_REDEPLOY_STAGE first_server_stopped_and_original_B_socket_actually_closed passed"
        );
        Ok(ClosedNormalRedeployState { state: self })
    }
}
impl ClosedNormalRedeployState {
    pub(in super::super) async fn verify_after_reopen(
        mut self,
        base: &str,
    ) -> ConformanceResult<()> {
        let proof = self.state.verify_after_reconnect(base).await;
        let closed = self.state.close().await;
        proof?;
        closed?;
        println!(
            "NORMAL_REDEPLOY_CASE same_files_reopened_full_rows_replays_and_stale_cutoff passed"
        );
        Ok(())
    }
}
