//! Central observations carry closed classifications, never request data,
//! assertions or identities.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CentralObservation {
    EngineAlarmNotPublished {
        failure: central_storage::StoreFailure,
    },
    EngineStoreDown {
        kind: central_engine_model::MachineKind,
        key: central_engine_model::InstanceKey,
        failure: central_storage::StoreFailure,
    },
    EngineItemFailed {
        kind: central_engine_model::MachineKind,
        key: central_engine_model::InstanceKey,
        failure: central_engine_model::ItemFailureClass,
        count: u32,
        retired: bool,
        /// The workspaces a notice unit failure affected; empty for machine subjects.
        workspaces: Vec<workspace_model::WorkspaceId>,
    },
    /// A closed refusal classification; never statement bytes, key ids or tokens.
    OrganizationStatementRefused {
        organization: fleet_identity_protocol::OrganizationId,
        refusal: crate::organization_authority::FleetOrganizationStatementRefusal,
    },
}
