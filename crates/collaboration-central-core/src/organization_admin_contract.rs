//! Closed answers from a verified organization admin operation.
use crate::{organization_authority::OrganizationChangeRecord, person_request::AdmissionRefusal};
use message_board::{Description, ResourceName, WorkspaceId, WorkspaceState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The verified admin view of the org-owned workspace directory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceView {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub state: WorkspaceState,
}

/// Verification failures become a bare 401; answered operations have a body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationAdminAnswer {
    Answered(OrganizationAdminResult),
    Unauthenticated,
}

/// The closed body of a 200 on an admin route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationAdminResult {
    Applied {
        workspace: WorkspaceView,
    },
    Found {
        workspace: Option<WorkspaceView>,
    },
    Records {
        records: Vec<OrganizationChangeRecord>,
    },
    Refused {
        reason: AdminRefusal,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdminRefusal {
    NameInUse,
    NotPermitted,
    SecondFactorRequired,
    RefreshRequired,
    AuthorizationSyncRequired,
    Expired,
    RequestTime,
    RequestReused,
    /// The same operation ID arrived with a different request.
    Conflict,
    Rejected,
}

impl From<AdmissionRefusal> for AdminRefusal {
    fn from(refusal: AdmissionRefusal) -> Self {
        match refusal {
            AdmissionRefusal::NotPermitted => Self::NotPermitted,
            AdmissionRefusal::SecondFactorRequired => Self::SecondFactorRequired,
            AdmissionRefusal::RefreshRequired => Self::RefreshRequired,
            AdmissionRefusal::AuthorizationSyncRequired => Self::AuthorizationSyncRequired,
            AdmissionRefusal::Expired => Self::Expired,
            AdmissionRefusal::RequestTime => Self::RequestTime,
            AdmissionRefusal::RequestReused => Self::RequestReused,
        }
    }
}
