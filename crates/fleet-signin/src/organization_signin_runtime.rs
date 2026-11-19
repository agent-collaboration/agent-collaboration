//! Binds sign-in to one runtime and its engine's alarm publication policy.
use central_storage::StoreFailure;
use fleet_contracts::{AlarmRecompute, OrganizationStamp};
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores, workspace_decode_error::corrupt};

use crate::{CentralOrganizationIdentity, OrganizationAuthority, prepare_central_alarm};

pub struct OrganizationSignIn<'runtime, Host: CentralStore, Stores: WorkspaceStores, Alarm> {
    pub(crate) runtime: &'runtime FleetRuntime<Host, Stores>,
    pub(crate) authority: &'runtime OrganizationAuthority,
    pub(crate) alarm: Alarm,
}

impl<'runtime, Host: CentralStore, Stores: WorkspaceStores, Alarm: AlarmRecompute>
    OrganizationSignIn<'runtime, Host, Stores, Alarm>
{
    pub fn new(
        runtime: &'runtime FleetRuntime<Host, Stores>,
        authority: &'runtime OrganizationAuthority,
        alarm: Alarm,
    ) -> Self {
        Self {
            runtime,
            authority,
            alarm,
        }
    }

    pub(crate) fn statement_host(&self) -> &Host {
        self.runtime.statement_host()
    }
}

impl<Host: CentralStore, Stores: WorkspaceStores, Alarm: AlarmRecompute> OrganizationStamp
    for OrganizationSignIn<'_, Host, Stores, Alarm>
{
    async fn organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        match prepare_central_alarm(self.statement_host(), &self.alarm).await? {
            CentralOrganizationIdentity::Unprovisioned => Ok(false),
            CentralOrganizationIdentity::Stamped { organization_id }
                if organization_id == *self.runtime.organization() =>
            {
                Ok(true)
            }
            CentralOrganizationIdentity::Stamped { .. } => Err(corrupt("organization_identity.id")),
        }
    }
}
