//! Physical SQLite classes and closed actor variants at Fleet's typed row boundary.
use crate::fleet_row_codec::{corrupt, required};
use durable_steps::StoreError;
use message_board::{RecordedActor, RouterComponent, SessionEndpointRef, SessionRef};

pub(crate) fn text(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<String, StoreError> {
    if required(class, field)? != "text" {
        return Err(corrupt(field, "expected SQLite TEXT"));
    }
    required(value, field)
}

pub(crate) fn optional_text(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<String>, StoreError> {
    match (value, required(class, field)?.as_str()) {
        (None, "null") => Ok(None),
        (Some(value), "text") => Ok(Some(value)),
        _ => Err(corrupt(field, "expected SQLite TEXT or NULL")),
    }
}

pub(crate) fn unsigned(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<u64, StoreError> {
    if required(class, field)? != "integer" {
        return Err(corrupt(field, "expected SQLite INTEGER"));
    }
    let value = required(value, field)?;
    let decoded = value
        .parse::<u64>()
        .map_err(|error| corrupt(field, error))?;
    if decoded.to_string() != value || i64::try_from(decoded).is_err() {
        return Err(corrupt(
            field,
            "expected a canonical nonnegative SQLite integer",
        ));
    }
    Ok(decoded)
}

pub(crate) fn canonical<T>(value: String, field: &str) -> Result<T, StoreError>
where
    T: TryFrom<String> + Clone + Into<String>,
    T::Error: std::fmt::Display,
{
    let decoded = T::try_from(value.clone()).map_err(|error| corrupt(field, error))?;
    if decoded.clone().into() != value {
        return Err(corrupt(field, "stored value would be normalized"));
    }
    Ok(decoded)
}

pub(crate) fn decimal(value: &str, field: &str) -> Result<u64, StoreError> {
    let decoded = value
        .parse::<u64>()
        .map_err(|error| corrupt(field, error))?;
    if decoded.to_string() != value {
        return Err(corrupt(field, "expected canonical unsigned decimal TEXT"));
    }
    Ok(decoded)
}

pub(crate) fn optional_millis(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<i64>, StoreError> {
    if value.is_none() && class.as_deref() == Some("null") {
        return Ok(None);
    }
    crate::fleet_row_codec::check_millis(value.clone(), class, field)?;
    required(value, field)?
        .parse()
        .map(Some)
        .map_err(|error| corrupt(field, error))
}

/// The flat SQL representation is classified into RecordedActor before use.
pub(crate) struct ActorColumns {
    pub kind: String,
    pub human: Option<String>,
    pub service: Option<String>,
    pub endpoint: Option<String>,
    pub session: Option<String>,
    pub component: Option<String>,
}
impl ActorColumns {
    pub fn encode(actor: &RecordedActor) -> Self {
        match actor {
            RecordedActor::Human { human_id } => Self {
                kind: "human".into(),
                human: Some(human_id.as_str().into()),
                service: None,
                endpoint: None,
                session: None,
                component: None,
            },
            RecordedActor::Session { session } => Self {
                kind: "session".into(),
                human: None,
                service: Some(session.endpoint.service_id.as_str().into()),
                endpoint: Some(session.endpoint.endpoint_id.as_str().into()),
                session: Some(session.session_id.as_str().into()),
                component: None,
            },
            RecordedActor::Router {
                component,
                installation,
            } => Self {
                kind: "router".into(),
                human: None,
                service: Some(installation.as_str().into()),
                endpoint: None,
                session: None,
                component: Some(
                    match component {
                        RouterComponent::SubscriptionDelivery => "subscription_delivery",
                        RouterComponent::ArchiveObservation => "archive_observation",
                    }
                    .into(),
                ),
            },
        }
    }
    pub fn decode(self) -> Result<RecordedActor, StoreError> {
        match (
            self.kind.as_str(),
            self.human,
            self.service,
            self.endpoint,
            self.session,
            self.component,
        ) {
            ("human", Some(human), None, None, None, None) => Ok(RecordedActor::Human {
                human_id: canonical(human, "actor.human")?,
            }),
            ("session", None, Some(service), Some(endpoint), Some(session), None) => {
                Ok(RecordedActor::Session {
                    session: SessionRef {
                        endpoint: SessionEndpointRef {
                            service_id: canonical(service, "actor.service")?,
                            endpoint_id: canonical(endpoint, "actor.endpoint")?,
                        },
                        session_id: canonical(session, "actor.session")?,
                    },
                })
            }
            ("router", None, Some(service), None, None, Some(component)) => {
                Ok(RecordedActor::Router {
                    installation: canonical(service, "actor.service")?,
                    component: match component.as_str() {
                        "subscription_delivery" => RouterComponent::SubscriptionDelivery,
                        "archive_observation" => RouterComponent::ArchiveObservation,
                        _ => return Err(corrupt("actor.component", "unknown Router component")),
                    },
                })
            }
            _ => Err(corrupt(
                "actor",
                "unknown kind or fields outside its active variant",
            )),
        }
    }
}
