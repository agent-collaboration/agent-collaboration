//! Classify published read failures before transport adapters erase evidence.
//! Only fixed code/kind/stage survive. Unknown envelopes fail closed and neither
//! private server messages nor arbitrary data become presentation diagnostics.
use crate::picker_runtime_status::NativeLiveUnavailableReason;
use collaboration_client::{
    ClientError,
    protocol::{AdapterOperationFailure, OperationEffect, OperationFailureKind},
};
use serde::Deserialize;
use serde_json::Value;

const OPERATION_FAILED: i64 = -32050;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryReadFailure {
    Unavailable(NativeLiveUnavailableReason),
    Invalid { code: Option<i64> },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum PublishedUnavailableKind {
    Unavailable,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum PublishedDiscoveryStage {
    Discovery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedProviderUnavailable {
    #[serde(rename = "kind")]
    _kind: PublishedUnavailableKind,
    #[serde(rename = "stage")]
    _stage: PublishedDiscoveryStage,
    #[serde(rename = "message")]
    _message: String,
}

impl InventoryReadFailure {
    pub fn from_client(error: &ClientError) -> Self {
        match error {
            ClientError::Timeout => Self::Unavailable(NativeLiveUnavailableReason::TimedOut),
            ClientError::Transport(_) => {
                Self::Unavailable(NativeLiveUnavailableReason::TransportUnavailable)
            }
            ClientError::Discovery { .. } => {
                Self::Unavailable(NativeLiveUnavailableReason::DiscoveryUnavailable)
            }
            ClientError::Rejected { code, data } => Self::published(*code, data.as_ref()),
            _ => Self::Invalid { code: None },
        }
    }

    pub fn tool_error(mut structured: Value) -> Self {
        let Some(fields) = structured.as_object_mut() else {
            return Self::Invalid { code: None };
        };
        if fields.remove("mcpResult").as_ref().and_then(Value::as_str) != Some("error") {
            return Self::Invalid { code: None };
        }
        // Full adapter failures retain their numeric code. An incomplete adapter
        // cannot fall through to the smaller published provider payload shape.
        if fields.contains_key("effect")
            || fields.contains_key("code")
            || fields.contains_key("data")
            || fields.contains_key("serviceKind")
        {
            let failure: AdapterOperationFailure = match serde_json::from_value(structured) {
                Ok(value) => value,
                Err(_) => return Self::Invalid { code: None },
            };
            let code = failure.code;
            if failure.kind != OperationFailureKind::Rejected
                || failure.stage != "discovery"
                || failure.effect != OperationEffect::None
            {
                return Self::Invalid { code };
            }
            let Some(code) = code else {
                return Self::Invalid { code: None };
            };
            Self::published(code, failure.data.as_ref())
        } else {
            Self::published(OPERATION_FAILED, Some(&structured))
        }
    }

    pub fn published(code: i64, data: Option<&Value>) -> Self {
        if code != OPERATION_FAILED {
            return Self::Invalid { code: Some(code) };
        }
        let valid = data.is_some_and(|data| {
            serde_json::from_value::<PublishedProviderUnavailable>(data.clone()).is_ok()
        });
        if valid {
            Self::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable)
        } else {
            Self::Invalid { code: Some(code) }
        }
    }

    pub fn into_client(self) -> ClientError {
        match self {
            Self::Invalid { code: Some(code) } => ClientError::Rejected { code, data: None },
            Self::Invalid { code: None } => {
                ClientError::Protocol("invalid source inventory response")
            }
            Self::Unavailable(NativeLiveUnavailableReason::TimedOut) => ClientError::Timeout,
            Self::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable) => {
                ClientError::Rejected {
                    code: OPERATION_FAILED,
                    data: Some(
                        serde_json::json!({"kind":"unavailable","stage":"discovery","message":"Provider inventory unavailable"}),
                    ),
                }
            }
            Self::Unavailable(reason) => ClientError::Transport(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                reason.description(),
            )),
        }
    }
}
