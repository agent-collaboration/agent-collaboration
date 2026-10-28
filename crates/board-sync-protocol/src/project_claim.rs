//! Installation-authorized claim outcomes retain the native lease and copy identities.
use fleet_sync_protocol::{
    Epoch, PeerId, ReplicationPosition, SchemaIdentity, SyncTime, TransferId,
};
use message_board::{ProjectId, ProtocolVersion, RequestId};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectClaimRequest {
    pub request_id: RequestId,
    pub protocol_version: ProtocolVersion,
    pub project_id: ProjectId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectClaimReply {
    Claimed {
        #[schemars(schema_with = "epoch_schema")]
        epoch: Epoch,
        #[schemars(with = "i64")]
        expires_at: SyncTime,
        #[schemars(schema_with = "replication_position_schema")]
        start: ReplicationPosition,
    },
    Pending {
        attempt: RequestId,
    },
    Refused(ProjectClaimRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectClaimRefusal {
    AlreadyHeld {
        #[schemars(schema_with = "peer_schema")]
        holder: PeerId,
    },
    TransferInProgress {
        #[schemars(with = "message_board::TransferId")]
        transfer: TransferId,
    },
    SchemaTooNew {
        #[schemars(schema_with = "schema_identity_schema")]
        store: SchemaIdentity,
        supported: Option<u64>,
    },
    NotFound,
    Retired,
    Archived,
}

// Upstream owns validation and serialization. These schema functions describe
// its wire shapes without changing a PeerId to ServiceId or a copy sequence to
// a board feed position. Contract tests keep the native serialization explicit.
fn epoch_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"integer","minimum":1,"maximum":u64::MAX})
}
fn peer_schema(generator: &mut SchemaGenerator) -> Schema {
    <message_board::ProjectId as JsonSchema>::json_schema(generator)
}
fn replication_position_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "type":"object","additionalProperties":false,"required":["epoch","sequence"],
        "properties":{"epoch":{"type":"integer","minimum":1,"maximum":u64::MAX},
                      "sequence":{"type":"integer","minimum":0,"maximum":u64::MAX}}
    })
}
fn schema_identity_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "type":"object","additionalProperties":false,"required":["version","fingerprint"],
        "properties":{"version":{"type":"integer","minimum":0,"maximum":u64::MAX},
                      "fingerprint":{"type":"string","minLength":1}}
    })
}
