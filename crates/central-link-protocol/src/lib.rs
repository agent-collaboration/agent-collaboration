//! The closed Router–central wire vocabulary, without a runtime or storage driver.
mod link_codec;
mod link_delivery;
mod link_renewal;
mod link_request;
mod request_status;

pub use request_status::{RequestRefusal, RequestStatusAnswer, RequestStatusQuery};

pub use link_codec::{DataFrame, FrameDirection, LinkFrame, decode_text, encode_text};
pub use link_delivery::{DeliveryAnswer, MessageCancellation, MessageOutcome, MessageRefusal};
pub use link_renewal::{
    InvalidMachineToken, MachineTokenText, TokenRenewalRefusal, TokenRenewalResult,
};
pub use link_request::{
    CorrelationId, LinkRequest, LinkRequestBatch, MAX_LINK_REQUEST_BATCH_SIZE, ReplyPayload,
    RequestPayload, RequestScope,
};

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid link JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{field} does not match the operation's request-id policy")]
    RequestIdPolicy { field: &'static str },
    #[error("batch must contain 1 to 64 writes with distinct correlation IDs")]
    InvalidBatch,
    #[error("frame is not allowed in this direction")]
    WrongDirection,
}
