#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum ClientError {
    #[error("central link is unavailable")]
    CentralUnavailable,
    #[error("event belongs to an obsolete connection attempt")]
    StaleConnection,
    #[error("request does not follow the write request-id policy")]
    InvalidRequest,
    #[error("Welcome is unexpected or names another machine")]
    InvalidWelcome,
    #[error("unexpected frame for the Router client")]
    UnexpectedFrame,
    #[error("correlation id is already in flight")]
    DuplicateCorrelation,
    #[error("reply names no in-flight request")]
    UnknownCorrelation,
    #[error("reply payload belongs to a different operation family")]
    WrongReplyFamily,
    #[error("reply arrived before the request entered the transport")]
    ReplyBeforeSend,
}
