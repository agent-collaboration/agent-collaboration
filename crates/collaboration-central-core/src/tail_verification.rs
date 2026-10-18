//! Registry's closed authority answer for a rejected-tail report.
use collaboration_replication_protocol::{Epoch, InstallationId, SessionRefusal};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum TailVerificationResult {
    Verified { current_holder: InstallationId },
    Refused(TailVerificationRefusal),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TailVerificationRefusal {
    NotWriter { current: Option<Epoch> },
}
impl From<TailVerificationRefusal> for SessionRefusal {
    fn from(value: TailVerificationRefusal) -> Self {
        match value {
            TailVerificationRefusal::NotWriter { current } => Self::NotWriter { current },
        }
    }
}
