//! The narrow port the machine-login decisions run over. A host's store reads
//! the facts a query names and applies the decision's changes in the same
//! transaction, so no decision acts on facts another one has since changed.
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, FormerServiceFacts, InstallationRow, MachineClaim,
    PriorApproval, StoredEnrollment,
};
use crate::identity_contract::{DeviceLoginRequestId, FormerServiceId};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId};
use fleet_sync_protocol::PeerId;
use message_board::WorkspaceId;
use std::future::Future;

/// The facts a decision asks for. Unasked parts come back empty.
#[derive(Clone, Debug, Default)]
pub struct EnrollmentQuery {
    pub installation: Option<InstallationId>,
    pub peer: Option<PeerId>,
    pub enrollment: Option<EnrollmentId>,
    pub request: Option<DeviceLoginRequestId>,
    pub former: Option<FormerServiceId>,
    pub workspaces: Vec<WorkspaceId>,
}

/// One organization's observable machine-login state for one query.
#[derive(Clone, Debug)]
pub struct EnrollmentFacts {
    /// The organization whose Fleet store this is.
    pub organization: OrganizationId,
    /// Any enrollment, in any state, ever: the first machine skips the
    /// second-factor check.
    pub organization_has_enrollment: bool,
    /// An earlier approval under the queried request ID.
    pub prior_approval: Option<PriorApproval>,
    pub enrollment: Option<StoredEnrollment>,
    /// The queried installation's row, or the queried enrollment's.
    pub installation: Option<InstallationRow>,
    /// That installation's active enrollment.
    pub active: Option<StoredEnrollment>,
    /// That installation's allowances, in every state.
    pub allowances: Vec<AllowanceRow>,
    pub claim: MachineClaim,
    /// The queried workspaces this organization doesn't have.
    pub missing_workspaces: Vec<WorkspaceId>,
    pub former: FormerServiceFacts,
}

/// A decision's answer and the writes it needs, applied together.
#[derive(Clone, Debug)]
pub struct Decided<T> {
    pub answer: T,
    pub changes: Vec<EnrollmentChange>,
    /// The decision's clock, for the rows' `updated_at`.
    pub written_at_ms: i64,
}

impl<T> Decided<T> {
    pub fn unchanged(answer: T) -> Self {
        Self {
            answer,
            changes: Vec::new(),
            written_at_ms: 0,
        }
    }

    pub fn writing(answer: T, changes: Vec<EnrollmentChange>, written_at_ms: i64) -> Self {
        Self {
            answer,
            changes,
            written_at_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MachineEnrollmentFailure {
    #[error("the organization's machine enrollments are unavailable")]
    Unavailable,
    #[error("the enrollment store could not confirm its commit")]
    OutcomeUnknown,
    #[error("the injected ID source must supply UUIDv7 identifiers")]
    InvalidIdSource,
    #[error("the request's identity cannot key a change record")]
    UnrecordableRequest,
    /// A request ID seen before, on a store that keeps no request body to
    /// compare it with: pending the owner's decision on a request fingerprint.
    #[error("a repeated device-login request cannot be compared on this store yet")]
    RepeatPendingOwner,
    /// A stored row failed its check; nothing was coerced or written.
    #[error("the enrollment store holds a row that fails its check")]
    CorruptStore,
    /// The changes would break a port invariant; nothing was applied.
    #[error("the decision's changes break the enrollment contract: {rule}")]
    ContractViolated { rule: &'static str },
}

/// Owned by one organization's Fleet object. Contract:
/// - at most one enrollment per installation is `Enrolled`, and it is the
///   installation's `active` one;
/// - an installation's generation moves only by one, and only in a transaction
///   that activates an enrollment by its first Grant or revokes one for its
///   owner;
/// - the changes apply in order, all or none.
pub trait MachineEnrollmentPort: Send + Sync {
    fn transact<T: Send + 'static>(
        &self,
        query: EnrollmentQuery,
        decide: impl FnOnce(&EnrollmentFacts) -> Decided<T> + Send + 'static,
    ) -> impl Future<Output = Result<T, MachineEnrollmentFailure>> + Send;
}
