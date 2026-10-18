use crate::{ConnectionId, ObjectCallId};
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum DispatchCarry {
    ForwardPost { post: ForwardedPost },
    ForwardDelivery { holder_last_seen: DateTime<Utc> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ForwardRouteResult {
    Route {
        holder: InstallationId,
        sender_label: String,
        holder_last_seen: DateTime<Utc>,
    },
    SenderHolds,
    NotHeld,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardDeliveryResult {
    Delivered,
    NoConnection,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub(crate) enum ForwardPhase {
    Routing,
    AwaitingHolder { expected_holder: InstallationId },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingForward {
    pub forward_id: ForwardId,
    pub frame_id: FrameId,
    pub connection_id: ConnectionId,
    pub project: ProjectId,
    pub origin: InstallationId,
    pub phase: ForwardPhase,
    pub deadline: DateTime<Utc>,
    pub call_id: ObjectCallId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveredForward {
    pub forward_id: ForwardId,
    pub origin: InstallationId,
    pub project: ProjectId,
    pub deadline: DateTime<Utc>,
}
