//! The shared access and row-decoding operations for workspace features.
#![forbid(unsafe_code)]

use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};

mod workspace_actor_identity;
mod workspace_admission;
mod workspace_change_records;
mod workspace_json_records;
mod workspace_row_codec;

pub use workspace_actor_identity::{ActorIdentityPlan, set_notice_cadence};
pub use workspace_change_records::StoredChange;
pub use workspace_row_codec::WorkspaceRows;

pub struct WorkspaceAccess<'runtime, Host: CentralStore, Stores: WorkspaceStores> {
    runtime: &'runtime FleetRuntime<Host, Stores>,
}

impl<'runtime, Host: CentralStore, Stores: WorkspaceStores>
    WorkspaceAccess<'runtime, Host, Stores>
{
    pub fn new(runtime: &'runtime FleetRuntime<Host, Stores>) -> Self {
        Self { runtime }
    }
}
