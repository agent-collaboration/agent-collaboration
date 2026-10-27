//! The fleet client asks the Host-owned writer runtime to drive durable registration.
use fleet_sync_writer::WriterRegistrationOutcome;
use message_board::ProjectId;
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectRegistrationState {
    Pending,
    Superseded,
    Outcome(WriterRegistrationOutcome),
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectRegistrationError {
    #[error("project registration runtime is unavailable: {message}")]
    Runtime { message: String },
}

pub type ProjectRegistrationFuture<'runtime> = Pin<
    Box<
        dyn Future<Output = Result<ProjectRegistrationState, ProjectRegistrationError>>
            + Send
            + 'runtime,
    >,
>;

pub trait ProjectRegistration: Send + Sync {
    fn start<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime>;

    /// Observe Ready without transferring the Host's registration ownership.
    fn wait_ready<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime>;
}
