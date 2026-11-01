//! Shared history availability is independent of inspection and lifecycle consumers.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderHistoryAvailability {
    Available,
    HistoryUnavailable,
}
