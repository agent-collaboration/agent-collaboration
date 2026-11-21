//! Live workspace admission, actor identities, and stored workspace reads.
#![forbid(unsafe_code)]

mod workspace_actor_identity;
mod workspace_admission;
pub mod workspace_change_records;
mod workspace_json_records;
pub mod workspace_row_codec;

pub use workspace_actor_identity::{ActorIdentityPlan, plan_notice_cadence, read_notice_cadence};

use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

/// Access to workspace authority and actor identities through the shared runtime.
pub struct WorkspaceAccess<'runtime, Host: CentralStore, Stores: WorkspaceStores> {
    execution: &'runtime FleetRuntime<Host, Stores>,
}

impl<'runtime, Host: CentralStore, Stores: WorkspaceStores>
    WorkspaceAccess<'runtime, Host, Stores>
{
    pub fn new(execution: &'runtime FleetRuntime<Host, Stores>) -> Self {
        Self { execution }
    }
}
