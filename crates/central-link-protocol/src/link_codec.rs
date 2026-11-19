use crate::{
    CorrelationId, DeliveryAnswer, LinkRequest, LinkRequestBatch, MachineTokenText, ProtocolError,
    ReplyPayload, RequestStatusAnswer, RequestStatusQuery, TokenRenewalResult,
};
use serde::{Deserialize, Serialize};
use workspace_ids::MachineId;
use workspace_ids::MessageId;
use workspace_model::{Caller, Timestamp, central_mail::MessageRecord};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDirection {
    ToCentral,
    ToRouter,
}

/// Heartbeats are literal text, so Cloudflare can answer them without waking Rust.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkFrame {
    Ping,
    Pong,
    Data(DataFrame),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DataFrame {
    Welcome {
        machine_id: MachineId,
        central_time: Timestamp,
    },
    Request {
        request: Box<LinkRequest>,
    },
    Batch {
        requests: LinkRequestBatch,
    },
    Reply {
        correlation_id: CorrelationId,
        result: Box<ReplyPayload>,
    },
    RequestStatus {
        correlation_id: CorrelationId,
        caller: Caller,
        query: RequestStatusQuery,
    },
    RequestStatusReply {
        correlation_id: CorrelationId,
        result: Box<RequestStatusAnswer>,
    },
    /// Central to the recipient's Router: a queued direct message, pushed at
    /// once when it is accepted and again on reconnect while inside its window.
    Deliver {
        message: Box<MessageRecord>,
    },
    /// The recipient's Router to central: its answer to a delivered message,
    /// sent on its minute flush (owner, record:218).
    Ack {
        message_id: MessageId,
        answer: DeliveryAnswer,
    },
    /// Router to central: a fresh machine token for the open connection.
    RenewMachineToken {
        correlation_id: CorrelationId,
        token: MachineTokenText,
    },
    /// Central to Router: the answer to one RenewMachineToken.
    MachineTokenRenewal {
        correlation_id: CorrelationId,
        result: TokenRenewalResult,
    },
}

impl LinkFrame {
    pub fn check_direction(&self, direction: FrameDirection) -> Result<(), ProtocolError> {
        let valid = match self {
            Self::Ping => direction == FrameDirection::ToCentral,
            Self::Pong => direction == FrameDirection::ToRouter,
            Self::Data(data) => match data {
                DataFrame::Request { .. }
                | DataFrame::Batch { .. }
                | DataFrame::RequestStatus { .. }
                | DataFrame::Ack { .. }
                | DataFrame::RenewMachineToken { .. } => direction == FrameDirection::ToCentral,
                DataFrame::Welcome { .. }
                | DataFrame::Reply { .. }
                | DataFrame::RequestStatusReply { .. }
                | DataFrame::Deliver { .. }
                | DataFrame::MachineTokenRenewal { .. } => direction == FrameDirection::ToRouter,
            },
        };
        if valid {
            Ok(())
        } else {
            Err(ProtocolError::WrongDirection)
        }
    }
}

pub fn decode_text(text: &str, direction: FrameDirection) -> Result<LinkFrame, ProtocolError> {
    let frame = match text {
        "ping" => LinkFrame::Ping,
        "pong" => LinkFrame::Pong,
        _ => LinkFrame::Data(serde_json::from_str(text)?),
    };
    frame.check_direction(direction)?;
    Ok(frame)
}

pub fn encode_text(frame: &LinkFrame, direction: FrameDirection) -> Result<String, ProtocolError> {
    frame.check_direction(direction)?;
    match frame {
        LinkFrame::Ping => Ok("ping".to_owned()),
        LinkFrame::Pong => Ok("pong".to_owned()),
        LinkFrame::Data(data) => Ok(serde_json::to_string(data)?),
    }
}

#[cfg(test)]
#[path = "link_codec_test.rs"]
mod tests;
