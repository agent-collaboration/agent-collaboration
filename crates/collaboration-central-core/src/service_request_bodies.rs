//! Decode only the operation selected by the authenticated C7 credential.
use crate::{
    AuthorizedServiceOperation, ServiceOperation, ServiceReceiverRefusal,
    identity_contract::{
        AuthorizeDeviceApproval, DecideDeviceLogin, DecideMachineToken, FindCentral,
        InitializeOrganization, PersonAdmissionRequest, SettleDeviceApproval,
    },
};
use fleet_identity_protocol::OrganizationId;
use serde::Deserialize;

// Narrow the existing common union without copying any producer-owned fields.
// Deserialization grants only the chosen variant; there is no fallback to a
// second operation when this operation's reader refuses the body.
#[derive(Deserialize)]
#[serde(try_from = "PersonAdmissionRequest")]
pub struct AdmitSignInBody(PersonAdmissionRequest);
impl TryFrom<PersonAdmissionRequest> for AdmitSignInBody {
    type Error = ServiceReceiverRefusal;
    fn try_from(request: PersonAdmissionRequest) -> Result<Self, Self::Error> {
        match request {
            PersonAdmissionRequest::AdmitSignIn { .. } => Ok(Self(request)),
            PersonAdmissionRequest::Standing { .. } => Err(ServiceReceiverRefusal::WrongOperation),
        }
    }
}
impl AdmitSignInBody {
    #[must_use]
    pub fn into_request(self) -> PersonAdmissionRequest {
        self.0
    }
}

#[derive(Deserialize)]
#[serde(try_from = "PersonAdmissionRequest")]
pub struct PersonStandingBody(PersonAdmissionRequest);
impl TryFrom<PersonAdmissionRequest> for PersonStandingBody {
    type Error = ServiceReceiverRefusal;
    fn try_from(request: PersonAdmissionRequest) -> Result<Self, Self::Error> {
        match request {
            PersonAdmissionRequest::Standing { .. } => Ok(Self(request)),
            PersonAdmissionRequest::AdmitSignIn { .. } => {
                Err(ServiceReceiverRefusal::WrongOperation)
            }
        }
    }
}
impl PersonStandingBody {
    #[must_use]
    pub fn into_request(self) -> PersonAdmissionRequest {
        self.0
    }
}

pub enum ServiceRequestBody {
    AdmitSignIn(AdmitSignInBody),
    PersonStanding(PersonStandingBody),
    CentralLookup(FindCentral),
    DeviceLoginDecisions(DecideDeviceLogin),
    MachineTokenDecisions(DecideMachineToken),
    DeviceApprovalAuthorize(AuthorizeDeviceApproval),
    DeviceApprovalActivate(SettleDeviceApproval),
    DeviceApprovalCancel(SettleDeviceApproval),
    InitializeOrganization(InitializeOrganization),
}
impl ServiceRequestBody {
    fn organization_id(&self) -> &OrganizationId {
        match self {
            Self::AdmitSignIn(body) => admission_organization(&body.0),
            Self::PersonStanding(body) => admission_organization(&body.0),
            Self::CentralLookup(body) => &body.organization_id,
            Self::DeviceLoginDecisions(body) => &body.organization_id,
            Self::MachineTokenDecisions(body) => &body.organization_id,
            Self::DeviceApprovalAuthorize(body) => &body.decision.organization_id,
            Self::DeviceApprovalActivate(body) | Self::DeviceApprovalCancel(body) => {
                &body.organization_id
            }
            Self::InitializeOrganization(body) => &body.organization_id,
        }
    }
}
fn admission_organization(request: &PersonAdmissionRequest) -> &OrganizationId {
    match request {
        PersonAdmissionRequest::AdmitSignIn {
            organization_id, ..
        }
        | PersonAdmissionRequest::Standing {
            organization_id, ..
        } => organization_id,
    }
}

/// Construction is private: trusted handlers receive only an authenticated,
/// decoded body whose organization is served by this receiver.
pub struct AuthorizedServiceRequest {
    body: ServiceRequestBody,
}
impl AuthorizedServiceRequest {
    #[must_use]
    pub fn into_body(self) -> ServiceRequestBody {
        self.body
    }
}

impl AuthorizedServiceOperation {
    pub fn decode_body(
        self,
        bytes: &[u8],
    ) -> Result<AuthorizedServiceRequest, ServiceReceiverRefusal> {
        let body = match self.operation() {
            ServiceOperation::AdmitSignIn => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::AdmitSignIn)
            }
            ServiceOperation::PersonStanding => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::PersonStanding)
            }
            ServiceOperation::CentralLookup => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::CentralLookup)
            }
            ServiceOperation::DeviceLoginDecisions => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::DeviceLoginDecisions)
            }
            ServiceOperation::MachineTokenDecisions => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::MachineTokenDecisions)
            }
            ServiceOperation::DeviceApprovalAuthorize => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::DeviceApprovalAuthorize)
            }
            ServiceOperation::DeviceApprovalActivate => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::DeviceApprovalActivate)
            }
            ServiceOperation::DeviceApprovalCancel => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::DeviceApprovalCancel)
            }
            ServiceOperation::InitializeOrganization => {
                serde_json::from_slice(bytes).map(ServiceRequestBody::InitializeOrganization)
            }
        }
        .map_err(|_| ServiceReceiverRefusal::WrongOperation)?;
        self.require_served_organization(body.organization_id())?;
        Ok(AuthorizedServiceRequest { body })
    }
}
