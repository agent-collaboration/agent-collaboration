//! Access observations carry classifications, never assertions or identities.

use crate::AccessRefusal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessRouteClass {
    Session,
    Admin,
    Control,
    Other,
}

impl AccessRouteClass {
    pub(crate) fn for_path(path: &str) -> Self {
        match path {
            "/v1/session" => Self::Session,
            "/admin/v1/conformance" => Self::Control,
            path if path.starts_with("/admin/v1/") => Self::Admin,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessKeySource {
    Cached,
    Fetched,
    Refetched,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessCheckOutcome {
    AcceptedMachine,
    AcceptedOwner,
    Refused(AccessRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CentralObservation {
    AccessCheck {
        route_class: AccessRouteClass,
        outcome: AccessCheckOutcome,
        key_source: Option<AccessKeySource>,
    },
}
