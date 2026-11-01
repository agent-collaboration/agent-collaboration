//! The existing machine-notice contract is independent of its Delivery runtime.
use board_sync_protocol::BoardNotice;
use durable_steps::CallTarget;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliverMachineNotice {
    pub installation: message_board::ServiceId,
}
impl CallTarget for DeliverMachineNotice {
    const KIND: &'static str = "central.machine-notice";
    type Request = BoardNotice;
    type Answer = ();
}
