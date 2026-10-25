//! Owner transfer inputs retain installation identity and native preview binding.
pub(crate) use collaboration_central_admin_protocol::{
    OwnerBlockedSubject, OwnerMove, OwnerPreviewSuccess, OwnerRetirement, OwnerRetirementAnswer,
    OwnerRetirementPhase, OwnerTakeover, OwnerTransferAnswer,
};
use durable_steps::CallTarget;
use fleet_sync_protocol::{DatabaseId, TransferId};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnerRegistryOperations {
    pub database: DatabaseId,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum OwnerRegistryCall {
    Preview,
    Move(OwnerMove),
    Takeover(OwnerTakeover),
    Retire(OwnerRetirement),
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum OwnerRegistryAnswer {
    Preview(OwnerPreviewAnswer),
    Transfer(OwnerTransferAnswer),
    Retirement(OwnerRetirementAnswer),
}
impl CallTarget for OwnerRegistryOperations {
    const KIND: &'static str = "central.owner-registry";
    type Request = OwnerRegistryCall;
    type Answer = OwnerRegistryAnswer;
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum OwnerPreviewAnswer {
    Ready(OwnerPreviewSuccess),
    NotFound,
    Retired,
}
#[derive(Clone)]
pub(crate) enum OwnerTransferOffer {
    Move(OwnerMove),
    Takeover(OwnerTakeover),
}
impl OwnerTransferOffer {
    pub(crate) fn attempt(&self) -> TransferId {
        match self {
            Self::Move(request) => request.transfer_id,
            Self::Takeover(request) => request.transfer_id,
        }
    }
    pub(crate) fn installation(&self) -> &ServiceId {
        match self {
            Self::Move(request) => &request.to_installation,
            Self::Takeover(request) => &request.to_installation,
        }
    }
}
