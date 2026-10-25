//! Retry only explicitly unsettled creation replies with the caller's unchanged request key.
use super::ProofResult;
use collaboration_client::{
    BoardClientError,
    board::{BoardFailureKind, RequestResult, WriteReply},
};

pub(super) async fn settled_creation<Call, Reply>(mut call: Call) -> ProofResult<WriteReply>
where
    Call: FnMut() -> Reply,
    Reply: std::future::Future<Output = Result<WriteReply, BoardClientError>>,
{
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                match call().await {
                    Ok(reply) if matches!(reply.result, RequestResult::PendingUnreachable) => {}
                    Err(BoardClientError::Rejected(failure))
                        if failure.kind == BoardFailureKind::BoardUnavailable => {}
                    result => return result,
                }
                tokio::task::yield_now().await;
            }
        })
        .await??,
    )
}
