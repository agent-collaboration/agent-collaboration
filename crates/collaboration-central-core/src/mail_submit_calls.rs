//! Core-only mail calls carry provenance separately from immutable content.

use crate::AdmissionOrigin;
use board_sync_protocol::{BoardDeliveryTypes, MailSubmitAnswer};
use durable_steps::CallTarget;
use fleet_sync_protocol::{DeliveryAnswer, DeliveryEnvelope, Epoch, PeerId};
use fleet_sync_session::SessionKey;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct MailSubmitRegistry;
impl CallTarget for MailSubmitRegistry {
    const KIND: &'static str = "central.mail-submit-registry";
    type Request = MailSubmitRequest;
    type Answer = MailSubmitReply;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MailSubmitRequest {
    pub session: SessionKey,
    pub origin: AdmissionOrigin,
    pub item: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
    pub origin_epoch: Option<Epoch>,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum MailSubmitReply {
    Answer(MailSubmitAnswer),
    InvalidShape { field: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MailboxSubmit {
    pub installation: ServiceId,
    pub recipient: PeerId,
}
impl CallTarget for MailboxSubmit {
    const KIND: &'static str = "central.mailbox-submit";
    type Request = MailboxSubmission;
    type Answer = DeliveryAnswer<BoardDeliveryTypes>;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MailboxSubmission {
    pub origin: PeerId,
    pub route: ServiceId,
    pub rank: Option<u64>,
    pub item: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
}
