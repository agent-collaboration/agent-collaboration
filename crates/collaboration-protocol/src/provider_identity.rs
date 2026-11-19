//! Provider actor identity with a byte-compatible HarnessSessionRef representation.

use crate::{EndpointId, EndpointRef, HarnessSessionRef, SessionId};
use message_board::{HumanId, Identity as BoardIdentity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Session serializes as the existing HarnessSessionRef JSON. Human IDs have a
/// distinct object shape, so old HarnessSessionRef-only readers fail closed.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderIdentity {
    Session(HarnessSessionRef),
    Human {
        #[serde(rename = "humanId")]
        human_id: HumanId,
    },
}

impl From<HarnessSessionRef> for ProviderIdentity {
    fn from(session: HarnessSessionRef) -> Self {
        Self::Session(session)
    }
}

impl ProviderIdentity {
    #[must_use]
    pub fn session(&self) -> Option<&HarnessSessionRef> {
        match self {
            Self::Session(session) => Some(session),
            Self::Human { .. } => None,
        }
    }

    pub fn to_board_identity(&self) -> Result<BoardIdentity, &'static str> {
        match self {
            Self::Session(session) => Ok(BoardIdentity::Session {
                session: message_board::SessionRef {
                    endpoint: message_board::SessionEndpointRef {
                        service_id: message_board::ServiceId::try_from(String::from(
                            session.endpoint.service_id.clone(),
                        ))
                        .map_err(|_| "invalid board service ID")?,
                        endpoint_id: message_board::EndpointId::try_from(String::from(
                            session.endpoint.endpoint_id.clone(),
                        ))
                        .map_err(|_| "invalid board endpoint ID")?,
                    },
                    session_id: message_board::SessionId::try_from(String::from(
                        session.session_id.clone(),
                    ))
                    .map_err(|_| "invalid board Session ID")?,
                },
            }),
            Self::Human { human_id } => Ok(BoardIdentity::Human {
                human_id: human_id.clone(),
            }),
        }
    }

    pub fn from_board_identity(identity: &BoardIdentity) -> Result<Self, &'static str> {
        match identity {
            BoardIdentity::Session { session } => Ok(Self::Session(HarnessSessionRef {
                endpoint: EndpointRef {
                    service_id: workspace_ids::MachineId::try_from(
                        session.endpoint.service_id.as_str().to_owned(),
                    )
                    .map_err(|_| "invalid provider actor service ID")?,
                    endpoint_id: EndpointId::try_from(
                        session.endpoint.endpoint_id.as_str().to_owned(),
                    )
                    .map_err(|_| "invalid provider actor endpoint ID")?,
                },
                session_id: SessionId::try_from(session.session_id.as_str().to_owned())
                    .map_err(|_| "invalid provider actor Session ID")?,
            })),
            BoardIdentity::Human { human_id } => Ok(Self::Human {
                human_id: human_id.clone(),
            }),
        }
    }
}
