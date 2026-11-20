use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineConnectionRefusal {
    UnknownMachine {},
    EnrollmentNotActive {},
    Superseded {},
    MachineInAnotherOrganization {},
    WrongClient {},
}
