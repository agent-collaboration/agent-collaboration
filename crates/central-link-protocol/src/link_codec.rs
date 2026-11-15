use crate::{CorrelationId, LinkRequest, ProtocolError, ReplyPayload};
use serde::{Deserialize, Serialize};
use workspace_ids::MessageId;
use workspace_model::{MachineId, Timestamp, central_mail::MessageRecord};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    Reply {
        correlation_id: CorrelationId,
        result: Box<ReplyPayload>,
    },
    Deliver {
        message: Box<MessageRecord>,
    },
    Ack {
        message_id: MessageId,
    },
}

impl LinkFrame {
    pub fn check_direction(&self, direction: FrameDirection) -> Result<(), ProtocolError> {
        let valid = match self {
            Self::Ping => direction == FrameDirection::ToCentral,
            Self::Pong => direction == FrameDirection::ToRouter,
            Self::Data(data) => match data {
                DataFrame::Request { .. } | DataFrame::Ack { .. } => {
                    direction == FrameDirection::ToCentral
                }
                DataFrame::Welcome { .. } | DataFrame::Reply { .. } | DataFrame::Deliver { .. } => {
                    direction == FrameDirection::ToRouter
                }
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
