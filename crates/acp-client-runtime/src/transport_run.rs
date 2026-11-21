//! Exact route identity shared by the ordered transport and callback routing.

use provider_session_machines::{
    SessionGeneration,
    session_owner::{DispatchAuthorization, OwnerIncarnation},
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TransportRun {
    pub incarnation: OwnerIncarnation,
    pub generation: SessionGeneration,
}

impl TransportRun {
    pub(crate) fn of(authorization: &DispatchAuthorization) -> Self {
        Self {
            incarnation: authorization.incarnation,
            generation: authorization.run,
        }
    }
}
