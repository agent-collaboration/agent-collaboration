//! Requester-side port for the Fleet's call-local remote-create admission and reads.
use collaboration_mail_protocol::{RemoteCreateAdmission, RemoteCreateRequest, RemoteCreateStatus};
use message_board::RequestId;
use std::{future::Future, pin::Pin};

pub type RemoteCreateFleetFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RemoteCreateFleetError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RemoteCreateFleetError {
    #[error("Fleet request was not sent")]
    NotSent,
    #[error("Fleet request answer was lost")]
    AnswerLost,
    #[error("Fleet proved this attempt made no new effect")]
    Retryable,
    #[error("Fleet returned an invalid remote-create answer")]
    InvalidReply,
    #[error("Remote-create call could not be encoded")]
    InvalidRequest,
}

/// One attempt on the existing machine session. The requester owns bounded retry.
pub trait RemoteCreateFleet: Send + Sync {
    fn request_remote_create(
        &self,
        request: RemoteCreateRequest,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateAdmission>;
    fn remote_create_status(
        &self,
        request: RequestId,
    ) -> RemoteCreateFleetFuture<'_, RemoteCreateStatus>;
}
