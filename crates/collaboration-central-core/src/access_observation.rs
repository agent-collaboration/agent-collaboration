//! Observations carry classifications, never assertions, statements or tokens.

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
    /// A Fleet refused an organization statement. Only the closed refusal kind
    /// and the organization the Fleet serves: never statement bytes, digests,
    /// key IDs or tokens.
    OrganizationStatementRefused {
        organization: fleet_identity_protocol::OrganizationId,
        refusal: crate::organization_authority::FleetOrganizationStatementRefusal,
    },
}
