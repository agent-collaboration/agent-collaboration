//! Registry lifecycle calls use the mailbox's journal, beside library-owned mail rows.

use durable_steps::CallTarget;
use fleet_sync_protocol::PeerId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClosePeerMailbox {
    pub peer: PeerId,
}
impl CallTarget for ClosePeerMailbox {
    const KIND: &'static str = "central.close-peer-mailbox";
    type Request = ();
    type Answer = ();
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenPeerMailbox {
    pub peer: PeerId,
}
impl CallTarget for OpenPeerMailbox {
    const KIND: &'static str = "central.open-peer-mailbox";
    type Request = ();
    type Answer = ();
}
