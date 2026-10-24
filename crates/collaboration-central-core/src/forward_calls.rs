//! Closed calls for routing a post and returning its transient opaque answer.
use board_sync_protocol::VerifiedForwardedPost;
use durable_steps::CallTarget;
use fleet_sync_protocol::{FrameId, PeerId};
use fleet_sync_session::SessionKey;
use message_board::{ActorOrigin, ProjectId, ServiceId};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct ForwardRoute;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForwardRouteRequest {
    pub session: SessionKey,
    pub project: ProjectId,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ForwardRouteResult {
    Route {
        holder: PeerId,
        installation: ServiceId,
        origin: ActorOrigin,
        holder_last_seen: Option<SyncTime>,
    },
    SenderHolds,
    NotHeld,
}
impl CallTarget for ForwardRoute {
    const KIND: &'static str = "central.forward-route";
    type Request = ForwardRouteRequest;
    type Answer = ForwardRouteResult;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliverForward {
    pub installation: ServiceId,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForwardDeliveryRequest {
    pub forward_frame: FrameId,
    pub holder: PeerId,
    pub deadline: SyncTime,
    pub post: VerifiedForwardedPost,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ForwardDeliveryResult {
    Delivered,
    NoConnection,
}
impl CallTarget for DeliverForward {
    const KIND: &'static str = "central.deliver-forward";
    type Request = ForwardDeliveryRequest;
    type Answer = ForwardDeliveryResult;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AnswerForward {
    pub installation: ServiceId,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForwardAnswerRequest {
    pub forward_frame: FrameId,
    pub project: ProjectId,
    pub reply: board_sync_protocol::BoardPayload,
}
impl CallTarget for AnswerForward {
    const KIND: &'static str = "central.answer-forward";
    type Request = ForwardAnswerRequest;
    type Answer = ();
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallationPeerRegistry {
    pub installation: ServiceId,
}
impl CallTarget for InstallationPeerRegistry {
    const KIND: &'static str = "central.installation-peer";
    type Request = ();
    type Answer = Option<PeerId>;
}
