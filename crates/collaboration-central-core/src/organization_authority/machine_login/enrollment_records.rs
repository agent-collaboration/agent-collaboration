//! What an organization's Fleet store holds for machine login, as the decisions
//! see it, and the closed set of changes a decision may make to it.
use crate::identity_contract::{
    AllowanceState, ClientId, DecideDeviceLogin, EndedMachineClient, EnrollmentRevocationReason,
    FormerServiceId,
};
use crate::organization_authority::OrganizationChangeRecord;
use durable_steps::Uuid;
use fleet_identity_protocol::{EnrollmentId, InstallationId, UnixSeconds};
use message_board::WorkspaceId;

/// Bumped by every first Grant that activates an enrollment and by every
/// owner revocation, and by nothing else (BA1).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct EnrollmentGeneration(pub u64);

impl EnrollmentGeneration {
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// An enrollment's lifecycle. `Approved` is internal: it admits nothing and
/// shows in no lookup until its first Grant makes it `Enrolled`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoredEnrollmentState {
    Approved,
    /// Approved, never granted within the device-code lifetime. Internal and terminal.
    Lapsed,
    Enrolled {
        machine_client_id: ClientId,
        last_grant_at: UnixSeconds,
    },
    Revoked {
        machine_client_id: ClientId,
        reason: EnrollmentRevocationReason,
    },
    Expired {
        machine_client_id: ClientId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredEnrollment {
    pub id: EnrollmentId,
    pub installation: InstallationId,
    /// The approval as the sign-in service sent it: a repeat of its request ID
    /// must carry the same body.
    pub approval: DecideDeviceLogin,
    /// The installation's generation when this enrollment was approved.
    pub base_generation: EnrollmentGeneration,
    /// Central's own clock in the approving transaction; an approval lapses
    /// a device-code lifetime after it.
    pub approved_at_ms: i64,
    pub state: StoredEnrollmentState,
}

impl StoredEnrollment {
    /// The machine client this enrollment ended with, once it had one.
    #[must_use]
    pub fn ended_client(&self) -> Option<EndedMachineClient> {
        match &self.state {
            StoredEnrollmentState::Approved | StoredEnrollmentState::Lapsed => None,
            StoredEnrollmentState::Enrolled {
                machine_client_id, ..
            }
            | StoredEnrollmentState::Revoked {
                machine_client_id, ..
            }
            | StoredEnrollmentState::Expired { machine_client_id } => Some(EndedMachineClient {
                enrollment_id: self.id.clone(),
                client_id: machine_client_id.clone(),
            }),
        }
    }
}

/// One installation's fence and its active enrollment, if any.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationRow {
    pub installation: InstallationId,
    pub generation: EnrollmentGeneration,
    pub active: Option<EnrollmentId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowanceRow {
    pub id: Uuid,
    pub installation: InstallationId,
    pub workspace: WorkspaceId,
    pub state: AllowanceState,
}

/// Whether another organization already holds this machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineClaim {
    Unclaimed,
    ThisOrganization,
    AnotherOrganization,
}

/// What this organization knows of a device login's former service id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormerServiceFacts {
    NotAsked,
    /// No installation here has that id, and none was linked from it.
    NamesNothing,
    /// An installation here is still keyed by the former id: its row and allowances.
    Unconverted {
        row: InstallationRow,
        allowances: Vec<AllowanceRow>,
    },
    /// Already converted, to this UUIDv7 installation.
    LinkedTo {
        installation: InstallationId,
    },
}

/// The closed set of writes. The port applies a decision's list in order, in
/// one transaction, or none of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollmentChange {
    InsertEnrollment(StoredEnrollment),
    /// A state transition of an existing enrollment.
    ReplaceEnrollment(StoredEnrollment),
    PutInstallation(InstallationRow),
    PutAllowance(AllowanceRow),
    /// Move every row keyed by the former installation to the new one.
    RekeyInstallation {
        from: InstallationId,
        to: InstallationId,
    },
    LinkFormerService {
        former: FormerServiceId,
        installation: InstallationId,
    },
    Record(OrganizationChangeRecord),
}
