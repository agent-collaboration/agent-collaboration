//! Core composes timer request plans inside the mail turn.
use crate::WorkspaceRuntime;
use central_storage::{GuardedStatement, StoreFailure};
use fleet_runtime_ports::{CentralStore, WorkspaceStores, workspace_decode_error::corrupt};
use workspace_model::{Timestamp, central_mail as mail};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn plan_timer_request(
        &self,
        operation: &mail::CentralMailOperation,
        now: Timestamp,
    ) -> Result<
        Result<(Vec<GuardedStatement>, mail::CentralMailAnswer), mail::CentralMailRefusal>,
        StoreFailure,
    > {
        use mail::{CentralMailOperation as Op, CentralMailRefusal as Refusal};
        match operation {
            Op::SetWake {
                target,
                due_at,
                text,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                fleet_wakes::plan_set_wake(target, *due_at, text, now, || self.execution.mint())
            }
            Op::CancelWake { wake_id } => {
                let rows = self
                    .execution
                    .statement_host()
                    .read(fleet_wakes::cancel_wake_read(wake_id))
                    .await?;
                fleet_wakes::plan_cancel_wake(wake_id, now, &rows)
            }
            Op::SetAutomation {
                target,
                timing,
                text,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                fleet_schedules::plan_set_automation(target, timing, text, now, || {
                    self.execution.mint()
                })
            }
            Op::DisableAutomation { automation_id } => {
                let rows = self
                    .execution
                    .statement_host()
                    .read(fleet_schedules::disable_automation_read(automation_id))
                    .await?;
                fleet_schedules::plan_disable_automation(automation_id, now, &rows)
            }
            Op::SendMessage { .. } | Op::SetNoticeCadence { .. } => Err(corrupt("timer operation")),
        }
    }
}
