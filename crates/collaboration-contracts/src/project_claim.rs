//! The Host owns claim transport and verified installation before admission opens.
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use std::{future::Future, pin::Pin};

#[derive(Debug, thiserror::Error)]
pub enum ProjectClaimError {
    #[error("project claim runtime is unavailable: {message}")]
    Runtime { message: String },
}
pub type ProjectClaimFuture<'runtime> =
    Pin<Box<dyn Future<Output = Result<ProjectClaimReply, ProjectClaimError>> + Send + 'runtime>>;
pub trait ProjectClaim: Send + Sync {
    fn claim<'runtime>(
        &'runtime self,
        request: &'runtime ProjectClaimRequest,
    ) -> ProjectClaimFuture<'runtime>;
}
