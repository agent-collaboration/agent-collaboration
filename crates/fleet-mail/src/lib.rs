//! Central mail and workspace draining on the shared runtime.
#![forbid(unsafe_code)]

use fleet_contracts::{AlarmRecompute, EngineAttemptControl};
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_workspace_access::WorkspaceAccess;

mod central_mail_records;
mod central_mail_requests;
mod central_mail_store;
mod central_mail_time;
mod central_timer_plans;
mod notification_windows;
mod workspace_drain;
mod workspace_notice_plans;

pub use workspace_notice_plans::DrainResult;

fn exact(
    sql: &'static str,
    args: Vec<central_storage::SqlValue>,
) -> central_storage::GuardedStatement {
    central_storage::GuardedStatement {
        statement: central_storage::Statement { sql, args },
        expect: central_storage::RowExpectation::Exactly(1),
    }
}

pub struct CentralMail<'runtime, Host: CentralStore, Stores: WorkspaceStores, Control> {
    runtime: &'runtime FleetRuntime<Host, Stores>,
    engine: Control,
}

impl<
    'runtime,
    Host: CentralStore,
    Stores: WorkspaceStores,
    Control: EngineAttemptControl + AlarmRecompute,
> CentralMail<'runtime, Host, Stores, Control>
{
    pub fn new(runtime: &'runtime FleetRuntime<Host, Stores>, engine: Control) -> Self {
        Self { runtime, engine }
    }

    fn workspace_access(&self) -> WorkspaceAccess<'_, Host, Stores> {
        WorkspaceAccess::new(self.runtime)
    }

    fn statement_host(&self) -> &Host {
        self.runtime.statement_host()
    }
}
