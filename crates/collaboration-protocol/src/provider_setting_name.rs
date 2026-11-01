//! Shared provider-setting names stay independent of request and setup-outcome contracts.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderSettingName {
    Mode,
    Model,
    Effort,
}
