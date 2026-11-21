//! Fold workspace changes into window updates and queued-notice edits.
use crate::{
    NoticeMailbox,
    notice_actor::route_notice_actor,
    notice_recipients::targeted_recipients,
    recipient_cadence::notice_due,
    window_rows::{NotificationWindow, notification_windows},
    window_statements::{delete_empty_windows, delete_window_items_for_thread, window_statements},
};
use central_storage::{GuardedStatement, StoreFailure};
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores, workspace_decode_error::corrupt};
use fleet_workspace_access::{WorkspaceAccess, workspace_row_codec::WorkspaceRows};
use workspace_model as m;
use workspace_model::{Timestamp, WorkspaceId};

pub struct WorkspaceNoticePlan {
    pub statements: Vec<GuardedStatement>,
    pub window_items_added: usize,
    pub skipped_recipients: usize,
}

/// The caller holds the central turn and supplies the mailbox for this execution.
pub async fn plan_workspace_notices_held<Host: CentralStore, Stores: WorkspaceStores>(
    execution: &FleetRuntime<Host, Stores>,
    mailbox: &impl NoticeMailbox,
    workspace: &WorkspaceId,
    rows: &WorkspaceRows,
    now: Timestamp,
) -> Result<WorkspaceNoticePlan, StoreFailure> {
    mailbox.validate_queued_notices_held().await?;
    let host = execution.statement_host();
    let access = WorkspaceAccess::new(execution);
    let mut windows = notification_windows(host).await?;
    let mut plan = WorkspaceNoticePlan {
        statements: Vec::new(),
        window_items_added: 0,
        skipped_recipients: 0,
    };
    let mut changes: Vec<_> = rows.changes.iter().collect();
    changes.sort_by_key(|change| change.position);
    for change in changes {
        let event = change.change()?.event;
        if let Some(thread) = &change.thread_id {
            let member = match &event {
                m::ChangeEvent::Left { member } | m::ChangeEvent::NotificationsOff { member } => {
                    Some(member)
                }
                _ => None,
            };
            if member.is_some() || matches!(event, m::ChangeEvent::Resolved) {
                plan.statements
                    .push(delete_window_items_for_thread(workspace, thread, member));
                plan.statements.extend(
                    mailbox
                        .plan_cancel_queued_notices_held(workspace, thread, member, now)
                        .await?,
                );
                continue;
            }
        }
        let mut recipients = targeted_recipients(change, rows)?;
        if let Some(thread) = &change.thread_id {
            let current = rows
                .threads
                .iter()
                .find(|row| &row.thread_id == thread)
                .ok_or_else(|| corrupt("workspace_changes.thread_id"))?;
            if current.state == m::ThreadState::Resolved {
                continue;
            }
            recipients.extend(
                rows.members
                    .iter()
                    .filter(|member| {
                        &member.thread_id == thread
                            && member.state == m::MembershipState::Joined
                            && member.notifications == m::NotificationSetting::On
                            && member.actor_identity_id != change.actor
                    })
                    .map(|member| member.actor_identity_id.clone()),
            );
        }
        for recipient in recipients {
            if route_notice_actor(&access, &recipient).await?.is_none() {
                plan.skipped_recipients += 1;
                continue;
            }
            let window =
                if let Some(window) = windows.iter().find(|window| window.recipient == recipient) {
                    window.clone()
                } else {
                    let first = change
                        .created_at
                        .ok_or_else(|| corrupt("workspace_changes.created_at"))?;
                    let window = NotificationWindow {
                        id: execution.mint()?,
                        recipient: recipient.clone(),
                        first,
                        due: notice_due(host, &recipient, now).await?,
                    };
                    windows.push(window.clone());
                    window
                };
            plan.statements.extend(window_statements(
                &window,
                workspace,
                change.thread_id.as_ref(),
                &change.change_id,
                execution.mint()?,
                now,
            ));
            plan.window_items_added += 1;
        }
    }
    plan.statements.push(delete_empty_windows());
    Ok(plan)
}
