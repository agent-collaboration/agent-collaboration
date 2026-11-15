//! The closed Router–central wire vocabulary, without a runtime or storage driver.
mod link_codec;
mod link_request;

pub use link_codec::{DataFrame, FrameDirection, LinkFrame, decode_text, encode_text};
pub use link_request::{CorrelationId, LinkRequest, ReplyPayload, RequestPayload};

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid link JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{field} does not match the operation's request-id policy")]
    RequestIdPolicy { field: &'static str },
    #[error("frame is not allowed in this direction")]
    WrongDirection,
}
