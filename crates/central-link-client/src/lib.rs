//! Event-driven client policy. The Host owns socket I/O and drives the injected-clock deadline.
mod error;
mod link_state;
mod pending_calls;

pub use link_state::{
    ClientAction, ClientEvent, ConnectionAttempt, LinkClient, LinkStatus, RefusalReason,
};
pub use pending_calls::{CallFailure, FailedCall, SendFailure};

pub use error::ClientError;
