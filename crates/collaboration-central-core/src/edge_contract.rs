//! The route narrows a verified caller once; its effect type carries that authority.
use crate::*;
use collaboration_replication_protocol::{InstallationId, ProjectId, ProjectImportPage};
#[derive(Clone, Debug, PartialEq)]
pub struct EnrolledInstallation {
    installation: InstallationId,
    expires_at: chrono::DateTime<chrono::Utc>,
}
impl EnrolledInstallation {
    pub fn from_credential(
        credential: CredentialStatus,
    ) -> Result<Self, collaboration_replication_protocol::CredentialRejection> {
        match credential {
            CredentialStatus::Machine {
                installation,
                expires_at,
            } => Ok(Self {
                installation,
                expires_at,
            }),
            CredentialStatus::Rejected(reason) => Err(reason),
        }
    }
    #[must_use]
    pub fn installation(&self) -> &InstallationId {
        &self.installation
    }
    #[must_use]
    pub fn credential(&self) -> CredentialStatus {
        CredentialStatus::Machine {
            installation: self.installation.clone(),
            expires_at: self.expires_at,
        }
    }
    #[must_use]
    pub fn effect(&self) -> MachineEdgeEffect {
        MachineEdgeEffect::OpenSession(self.installation.clone())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct OwnerIdentity(String);
impl OwnerIdentity {
    pub fn from_access(caller: AccessCaller) -> Result<Self, AccessRefusal> {
        match caller {
            AccessCaller::Owner { email } => Ok(Self(email)),
            AccessCaller::Machine { .. } => Err(AccessRefusal::WrongCallerClass),
        }
    }
    #[must_use]
    pub fn registry(&self, request: AdminRequest) -> OwnerEdgeEffect {
        OwnerEdgeEffect::Registry(RegistryAdminCall::Admin {
            owner: self.clone(),
            request,
        })
    }
    #[must_use]
    pub fn record_unheld(&self, unheld: UnheldProject) -> RegistryAdminCall {
        RegistryAdminCall::RecordUnheld {
            owner: self.clone(),
            unheld,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum VerifiedCaller {
    Machine(EnrolledInstallation),
    Owner(OwnerIdentity),
}
/// A machine route has no owner/admin effect in its result type.
/// ```
/// use collaboration_central_core::{EnrolledInstallation,MachineEdgeEffect};
/// fn route(caller:&EnrolledInstallation)->MachineEdgeEffect {caller.effect()}
/// ```
/// ```compile_fail
/// use collaboration_central_core::{EnrolledInstallation,OwnerEdgeEffect};
/// fn route(caller:&EnrolledInstallation)->OwnerEdgeEffect {caller.effect()}
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum MachineEdgeEffect {
    OpenSession(InstallationId),
}
#[derive(Clone, Debug, PartialEq)]
pub enum OwnerEdgeEffect {
    Registry(RegistryAdminCall),
    CopyImport(ProjectId, CopyImportCall),
    FleetImport(FleetImportCall),
    CopyOps(ProjectId, CopyOpsCall),
}
#[derive(Clone, Debug, PartialEq)]
pub enum RegistryAdminCall {
    Admin {
        owner: OwnerIdentity,
        request: AdminRequest,
    },
    RecordUnheld {
        owner: OwnerIdentity,
        unheld: UnheldProject,
    },
}
impl RegistryAdminCall {
    #[must_use]
    pub fn into_call(self) -> RegistryCall {
        match self {
            Self::Admin { owner, request } => RegistryCall::Admin {
                authority: AdminAuthority::Admin { subject: owner.0 },
                request,
            },
            Self::RecordUnheld { unheld, .. } => RegistryCall::RecordUnheld(unheld),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct CopyImportCall {
    pub upload: Box<ProjectImportPage>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FleetImportCall {
    pub bundle: Box<fleet_admission::FleetImportBundle>,
}
