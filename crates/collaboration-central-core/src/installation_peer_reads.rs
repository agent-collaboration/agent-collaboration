//! Session directory reads use the Registry's immutable enrollment bindings.
use board_sync_protocol::InstallationPeerPage;
use durable_steps::CallTarget;
use fleet_sync_protocol::PeerId;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

pub(crate) enum InstallationReadRequest {
    Lookup(ServiceId),
    Page(Option<PeerId>),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct InstallationPeerDirectory;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallationPeerPageRequest {
    pub after: Option<PeerId>,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum InstallationPeerPageRead {
    Page(InstallationPeerPage),
    CursorInvalid,
}

impl CallTarget for InstallationPeerDirectory {
    const KIND: &'static str = "central.installation-peers";
    type Request = InstallationPeerPageRequest;
    type Answer = InstallationPeerPageRead;
}
