use central_storage::StoreFailure;
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OrganizationProvisioningFailure {
    #[error("organization mismatch")]
    OrganizationMismatch,
    #[error("invalid organization statement")]
    InvalidStatement,
    #[error("stale organization statement")]
    StaleStatement,
    #[error("organization is unprovisioned")]
    Unprovisioned,
    #[error("organization floor missing")]
    OrganizationFloorMissing,
    #[error("organization sign-in unavailable")]
    Unavailable,
    #[error("organization sign-in outcome unknown")]
    OutcomeUnknown,
}
impl From<StoreFailure> for OrganizationProvisioningFailure {
    fn from(error: StoreFailure) -> Self {
        match error {
            StoreFailure::AnswerLost => Self::OutcomeUnknown,
            StoreFailure::Unavailable | StoreFailure::Corrupt { .. } => Self::Unavailable,
        }
    }
}
