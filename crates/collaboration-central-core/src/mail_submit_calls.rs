//! Core-only mail calls carry provenance separately from immutable content.

use crate::AdmissionOrigin;
use board_sync_protocol::{BoardDeliveryTypes, MailSubmitAnswer};
use durable_steps::CallTarget;
use fleet_protocol::{DeliveryEnvelope, Epoch};
use fleet_session::SessionKey;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetMailSubmit;
impl CallTarget for FleetMailSubmit {
    const KIND: &'static str = "central.mail-submit";
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
