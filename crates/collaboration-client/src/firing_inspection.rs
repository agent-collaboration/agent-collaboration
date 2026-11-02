//! Firing inspection requests; returned snapshots remain protocol-owned.
use collaboration_protocol::FiringId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FiringShowRequest {
    pub firing_id: FiringId,
}
