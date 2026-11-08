//! The narrow port the machine-login decisions run over. A host's store reads
//! the facts a query names and applies the decision's changes in the same
//! transaction, so no decision acts on facts another one has since changed.
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, FormerServiceFacts, InstallationRow, MachineClaim,
    StoredEnrollment,
};
use crate::identity_contract::{DeviceLoginRequestId, FormerServiceId};
use fleet_identity_protocol::{EnrollmentId, InstallationId};
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
    /// Any enrollment, in any state, ever: the first machine skips the
    /// second-factor check.
    pub organization_has_enrollment: bool,
    pub by_request: Option<StoredEnrollment>,
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
}

impl<T> Decided<T> {
    pub fn unchanged(answer: T) -> Self {
        Self {
            answer,
            changes: Vec::new(),
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
    fn transact<T: Send>(
        &self,
        query: EnrollmentQuery,
        decide: impl FnOnce(&EnrollmentFacts) -> Decided<T> + Send,
    ) -> impl Future<Output = Result<T, MachineEnrollmentFailure>> + Send;
}
