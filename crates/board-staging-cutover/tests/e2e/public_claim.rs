//! Retry only authoritative Pending/unready results with the frozen Claim identity.
use super::offline_cli_fixture::TestResult;
use board_staging_cutover::PreparedProjectBundle;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{BoardFailureKind, ProtocolVersion};

pub(super) async fn claim(
    client: &CollaborationClient,
    bundle: &PreparedProjectBundle,
) -> TestResult<ProjectClaimReply> {
    let request = ProjectClaimRequest {
        request_id: bundle.manifest().claim_request.clone(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: bundle.manifest().source.selected.project.clone(),
    };
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                match client.project_claim(request.clone()).await {
                    Ok(ProjectClaimReply::Pending { .. }) => tokio::task::yield_now().await,
                    Err(BoardClientError::Rejected(error))
                        if error.kind == BoardFailureKind::BoardUnavailable =>
                    {
                        tokio::task::yield_now().await
                    }
                    other => return other,
                }
            }
        })
        .await??,
    )
}
