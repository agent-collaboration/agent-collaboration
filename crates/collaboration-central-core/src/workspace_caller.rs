use crate::ValidatedMachineConnection;
use fleet_identity_protocol::OrganizationId;
use workspace_model::Caller;

/// A semantic caller bound to core-validated machine_id provenance.
#[derive(Clone)]
pub struct WorkspaceCaller {
    connection: ValidatedMachineConnection,
    caller: Caller,
}
impl WorkspaceCaller {
    pub fn new(
        connection: ValidatedMachineConnection,
        caller: Caller,
    ) -> Result<Self, workspace_model::RefusalReason> {
        if caller.machine_id() != connection.machine_id() {
            return Err(workspace_model::RefusalReason::MachineRevoked);
        }
        Ok(Self { connection, caller })
    }
    /// A public machine link carries only sessions bound to its validated machine.
    pub fn from_machine_link(
        connection: ValidatedMachineConnection,
        caller: Caller,
    ) -> Result<Self, workspace_model::RefusalReason> {
        if !matches!(caller, Caller::Session(_)) {
            return Err(workspace_model::RefusalReason::InvalidInput {
                field: "caller".to_owned(),
            });
        }
        Self::new(connection, caller)
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        self.connection.organization_id()
    }
    #[must_use]
    pub fn caller(&self) -> &Caller {
        &self.caller
    }
    #[must_use]
    pub fn connection(&self) -> &ValidatedMachineConnection {
        &self.connection
    }
}
