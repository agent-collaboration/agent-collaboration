//! Serialized edge requests; every receiver executes the shared asynchronous core.
use crate::{FleetCall, FleetResult, RegistryCall, RegistryResult};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WireCall {
    Registry(RegistryCall),
    Fleet(FleetCall),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WireResult {
    Registry(RegistryResult),
    Fleet(FleetResult),
}
