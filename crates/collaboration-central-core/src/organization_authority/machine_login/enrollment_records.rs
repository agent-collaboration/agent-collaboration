//! Facts decoded from one organization's owning store.
use crate::identity_contract::{
    AllowedWorkspaces, DeviceLoginRequestId, EndedMachineClient, EnrollmentRevocationReason,
    PersonId,
};
use crate::organization_authority::OrganizationChangeRecord;
use central_storage::StoreFailure;
use fleet_contracts::ClientId;
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use message_board::{MachineLabel, WorkspaceId};
use workspace_ids::MachineId;

/// A generation is an exact cross-host SQL integer; exhaustion never wraps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EnrollmentGeneration(u64);
impl TryFrom<u64> for EnrollmentGeneration {
    type Error = StoreFailure;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value > 9_007_199_254_740_991 {
            return Err(corrupt("login_machines.enrollment_generation"));
        }
        Ok(Self(value))
    }
}
impl EnrollmentGeneration {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
    pub(crate) fn next(self) -> Result<Self, StoreFailure> {
        Self::try_from(
            self.0
                .checked_add(1)
                .ok_or_else(|| corrupt("login_machines.enrollment_generation"))?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MachineState {
    Enrolled,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MachineRow {
    pub machine_id: MachineId,
    pub label: MachineLabel,
    pub allowed_workspaces: Vec<WorkspaceId>,
    pub generation: EnrollmentGeneration,
    pub state: MachineState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredEnrollmentState {
    Approved,
    Lapsed,
    Enrolled {
        client_id: ClientId,
        last_grant_at: UnixSeconds,
    },
    Revoked {
        client_id: ClientId,
        reason: EnrollmentRevocationReason,
    },
    Expired {
        client_id: ClientId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredEnrollment {
    pub id: EnrollmentId,
    pub machine_id: MachineId,
    pub request_id: DeviceLoginRequestId,
    pub approved_by: PersonId,
    pub label: MachineLabel,
    pub allowed_workspaces: AllowedWorkspaces,
    pub base_generation: EnrollmentGeneration,
    pub approved_at_ms: i64,
    pub code_expires_at: UnixSeconds,
    pub state: StoredEnrollmentState,
    pub ended_at_ms: Option<i64>,
}
impl StoredEnrollment {
    pub(crate) fn ended_client(&self) -> Option<EndedMachineClient> {
        let client_id = match &self.state {
            StoredEnrollmentState::Enrolled { client_id, .. }
            | StoredEnrollmentState::Revoked { client_id, .. }
            | StoredEnrollmentState::Expired { client_id } => client_id.clone(),
            StoredEnrollmentState::Approved | StoredEnrollmentState::Lapsed => return None,
        };
        Some(EndedMachineClient {
            enrollment_id: self.id.clone(),
            client_id,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EnrollmentQuery {
    pub machine_id: Option<MachineId>,
    pub request: Option<DeviceLoginRequestId>,
    pub enrollment: Option<EnrollmentId>,
    pub workspaces: Vec<WorkspaceId>,
}

#[derive(Clone, Debug)]
pub(crate) struct EnrollmentFacts {
    pub organization_id: OrganizationId,
    pub organization_has_enrollment: bool,
    pub prior_approval: Option<StoredEnrollment>,
    pub enrollment: Option<StoredEnrollment>,
    pub machine: Option<MachineRow>,
    pub active: Option<StoredEnrollment>,
    pub superseded_client: Option<EndedMachineClient>,
    pub missing_workspaces: Vec<WorkspaceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EnrollmentChange {
    InsertEnrollment(StoredEnrollment),
    ReplaceEnrollment(StoredEnrollment),
    PutMachine(MachineRow),
    Record(OrganizationChangeRecord),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Decided<T> {
    pub answer: T,
    pub changes: Vec<EnrollmentChange>,
    pub written_at_ms: Option<i64>,
}
impl<T> Decided<T> {
    pub(crate) fn unchanged(answer: T) -> Self {
        Self {
            answer,
            changes: Vec::new(),
            written_at_ms: None,
        }
    }
    pub(crate) fn writing(answer: T, changes: Vec<EnrollmentChange>, now_ms: i64) -> Self {
        Self {
            answer,
            changes,
            written_at_ms: Some(now_ms),
        }
    }
}
