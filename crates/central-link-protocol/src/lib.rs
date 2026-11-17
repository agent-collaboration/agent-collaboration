//! The closed Router–central wire vocabulary, without a runtime or storage driver.
mod link_codec;
mod link_request;

pub use link_codec::{DataFrame, FrameDirection, LinkFrame, decode_text, encode_text};
pub use link_request::{
    CorrelationId, LinkRequest, LinkRequestBatch, MAX_LINK_REQUEST_BATCH_SIZE, ReplyPayload,
    RequestPayload,
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
