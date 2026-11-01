//! The Host chooses the enrollment bridge or its machine-token launch owner for this connection.
use collaboration_contracts::HarnessSessionLaunchPort;
use collaboration_protocol::EndpointRef;
use std::sync::Arc;

#[derive(Clone)]
pub enum CodexHarnessAdmission {
    AccessEnrollment,
    MachineTokens {
        endpoint: EndpointRef,
        port: Arc<dyn HarnessSessionLaunchPort>,
    },
}
