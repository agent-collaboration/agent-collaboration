//! Describe, plan and apply Central's notice window whole unit.
use crate::{
    NoticeMailbox, notice_actor::route_notice_actor, window_items::window_notice_items,
    window_rows::notification_windows, window_statements::delete_window,
};
use central_engine_model::{EngineTime, InstanceKey, MachineKind, RetirementThreshold};
use central_storage::{AlarmUpdate, GuardedStatement, StatementBatch};
use fleet_contracts::{EngineSubject, LegacyUnitOutcome, MailDeliveryReady, ObservedControl};
use fleet_engine::WholeUnit;
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores, workspace_decode_error::corrupt};
use fleet_workspace_access::WorkspaceAccess;
use workspace_model::central_mail as mail;

pub fn window_unit() -> WholeUnit {
    WholeUnit {
        subject: EngineSubject {
            kind: MachineKind("legacy_notice_windows"),
            key: InstanceKey("all".into()),
        },
        threshold: RetirementThreshold::NOTICE_UNITS,
    }
}

pub struct DueWindowPlan<'store, Host: CentralStore> {
    store: &'store Host,
    statements: Vec<GuardedStatement>,
    deliveries: Vec<MailDeliveryReady>,
}

impl<Host: CentralStore> DueWindowPlan<'_, Host> {
    pub fn delivery_count(&self) -> usize {
        self.deliveries.len()
    }

    /// Apply under the same central turn used for planning.
    /// Release readiness hints only after the batch succeeds.
    pub async fn apply_held(self) -> Result<Vec<MailDeliveryReady>, LegacyUnitOutcome> {
        LegacyUnitOutcome::batch(
            self.store
                .run_batch(StatementBatch {
                    statements: self.statements,
                    alarm: AlarmUpdate::Unchanged,
                })
                .await,
        )?;
        Ok(self.deliveries)
    }
}

/// Plan due windows while holding the central turn and using a mailbox over
/// the same runtime. Pass the Engine's subject and observed control for this
/// exact window run.
///
/// Apply a successful plan once under that same turn, or abandon it without
/// reporting the unit complete.
pub async fn plan_due_windows_held<'store, Host: CentralStore, Stores: WorkspaceStores>(
    execution: &'store FleetRuntime<Host, Stores>,
    mailbox: &impl NoticeMailbox,
    now: EngineTime,
    subject: &EngineSubject,
    observed: &ObservedControl,
) -> Result<DueWindowPlan<'store, Host>, LegacyUnitOutcome> {
    let store = execution.statement_host();
    let access = WorkspaceAccess::new(execution);
    let mut statements = Vec::new();
    let mut deliveries = Vec::new();
    for window in notification_windows(store)
        .await?
        .into_iter()
        .filter(|window| window.due <= now)
    {
        let items = window_notice_items(execution, &window.id).await?;
        if let Some(target) = route_notice_actor(&access, &window.recipient).await? {
            let first = items
                .first()
                .ok_or_else(|| corrupt("mail_notification_window_items"))?;
            let source = mail::NoticeSource {
                change_id: first.change_id.clone(),
                recipient_actor: window.recipient.clone(),
                workspace_id: first.workspace_id.clone(),
                thread_id: first.thread_id.clone(),
            };
            let (insert, delivery) = mailbox.plan_sourced_notice(target, items, source, now)?;
            statements.push(insert);
            deliveries.push(delivery);
        }
        statements.extend(delete_window(&window.id));
    }
    match observed {
        ObservedControl::Absent => {}
        ObservedControl::Present(guard) => {
            statements.push(fleet_engine::delete_observed_attempt(subject, guard))
        }
    }
    Ok(DueWindowPlan {
        store,
        statements,
        deliveries,
    })
}
