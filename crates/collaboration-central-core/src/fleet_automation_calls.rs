//! The Session-to-Fleet Automation call contract carries the bound installation.
use board_sync_protocol::BoardPayload;
use collaboration_protocol::AutomationAnswer;
use durable_steps::CallTarget;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetAutomation;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FleetAutomationRequest {
    pub installation: ServiceId,
    pub call: BoardPayload,
}

impl CallTarget for FleetAutomation {
    const KIND: &'static str = "central.automation";
    type Request = FleetAutomationRequest;
    type Answer = AutomationAnswer;
}
