//! Classify optional Human/Session identity fields without admitting inactive payloads.
use crate::{fleet_column_codec::canonical, fleet_row_codec::corrupt};
use durable_steps::StoreError;
use message_board::{Identity, SessionEndpointRef, SessionRef};

#[derive(Default)]
pub(crate) struct IdentityColumns {
    pub kind: Option<String>,
    pub human: Option<String>,
    pub service: Option<String>,
    pub endpoint: Option<String>,
    pub session: Option<String>,
}
impl IdentityColumns {
    pub(crate) fn encode(identity: Option<Identity>) -> Self {
        match identity {
            None => Self::default(),
            Some(Identity::Human { human_id }) => Self {
                kind: Some("human".into()),
                human: Some(human_id.as_str().into()),
                ..Self::default()
            },
            Some(Identity::Session { session }) => Self {
                kind: Some("session".into()),
                human: None,
                service: Some(session.endpoint.service_id.as_str().into()),
                endpoint: Some(session.endpoint.endpoint_id.as_str().into()),
                session: Some(session.session_id.as_str().into()),
            },
        }
    }
    pub(crate) fn decode(self, field: &str) -> Result<Option<Identity>, StoreError> {
        match (
            self.kind.as_deref(),
            self.human,
            self.service,
            self.endpoint,
            self.session,
        ) {
            (None, None, None, None, None) => Ok(None),
            (Some("human"), Some(human), None, None, None) => Ok(Some(Identity::Human {
                human_id: canonical(human, field)?,
            })),
            (Some("session"), None, Some(service), Some(endpoint), Some(session)) => {
                Ok(Some(Identity::Session {
                    session: SessionRef {
                        endpoint: SessionEndpointRef {
                            service_id: canonical(service, field)?,
                            endpoint_id: canonical(endpoint, field)?,
                        },
                        session_id: canonical(session, field)?,
                    },
                }))
            }
            _ => Err(corrupt(field, "unknown identity kind or inactive fields")),
        }
    }
}
