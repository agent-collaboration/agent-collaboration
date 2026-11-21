use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Permission required for the private machine revocation command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum RevokePermission {
    #[serde(rename = "machine:revoke")]
    MachineRevoke,
}

impl RevokePermission {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::MachineRevoke => "machine:revoke",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AllowanceState {
    Active,
    Withdrawn,
}
