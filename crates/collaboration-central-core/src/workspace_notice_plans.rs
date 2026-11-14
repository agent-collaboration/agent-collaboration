//! Position-ordered activity joins one central recipient window across workspaces.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    guarded_statements::{any, text},
    notification_windows::{NotificationWindow, window_statements},
    workspace_decode_error::corrupt,
    workspace_row_codec::WorkspaceRows,
};
use central_storage::{GuardedStatement, StoreFailure};
use std::collections::BTreeSet;
use workspace_model as m;

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrainResult {
    pub through: m::ChangePosition,
    pub notices_queued: usize,
    pub window_items_added: usize,
    pub skipped_recipients: usize,
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn plan_workspace_notices(
        &self,
        workspace: &m::WorkspaceId,
        rows: &WorkspaceRows,
        now: m::Timestamp,
    ) -> Result<(Vec<GuardedStatement>, DrainResult), StoreFailure> {
        crate::central_mail_records::decode_messages(
            &self
                .host
                .read(crate::central_mail_records::notice_messages())
                .await?,
        )?;
        let mut windows = self.notification_windows().await?;
        let mut statements = Vec::new();
        let mut result = DrainResult {
            through: rows.high_water,
            ..Default::default()
        };
        let mut changes: Vec<_> = rows.changes.iter().collect();
        changes.sort_by_key(|change| change.position);
        for change in changes {
            let event = change.change()?.event;
            if let Some(thread) = &change.thread_id {
                let member = match &event {
                    m::ChangeEvent::Left { member }
                    | m::ChangeEvent::NotificationsOff { member } => Some(member),
                    _ => None,
                };
                if member.is_some() || matches!(event, m::ChangeEvent::Resolved) {
                    statements.push(any("DELETE FROM notification_window_items WHERE workspace_id=? AND thread_id=? AND (? IS NULL OR window_id IN (SELECT window_id FROM notification_windows WHERE recipient_actor=?))",vec![text(workspace),text(thread),member.map_or(central_storage::SqlValue::Null,text),member.map_or(central_storage::SqlValue::Null,text)]));
                    statements.extend(
                        self.cancel_ready_notices(workspace, thread, member, now)
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
                    .ok_or_else(|| corrupt("changes.thread_id"))?;
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
                if self.route_notice_actor(&recipient).await?.is_none() {
                    result.skipped_recipients += 1;
                    continue;
                }
                let window = if let Some(window) =
                    windows.iter().find(|window| window.recipient == recipient)
                {
                    window.clone()
                } else {
                    let first = change
                        .created_at
                        .ok_or_else(|| corrupt("changes.created_at"))?;
                    let window = NotificationWindow {
                        id: self.mint()?,
                        recipient: recipient.clone(),
                        first,
                        due: self.notice_due(&recipient, first).await?,
                    };
                    windows.push(window.clone());
                    window
                };
                statements.extend(window_statements(
                    &window,
                    workspace,
                    change.thread_id.as_ref(),
                    &change.change_id,
                    self.mint()?,
                    now,
                ));
                result.window_items_added += 1;
            }
        }
        statements.push(any("DELETE FROM notification_windows WHERE NOT EXISTS(SELECT 1 FROM notification_window_items i WHERE i.window_id=notification_windows.window_id)",vec![]));
        Ok((statements, result))
    }
}
fn targeted_recipients(
    change: &crate::workspace_change_records::StoredChange,
    rows: &WorkspaceRows,
) -> Result<BTreeSet<m::ActorIdentityId>, StoreFailure> {
    let mut recipients = BTreeSet::new();
    match change.change()?.event {
        m::ChangeEvent::ProjectRoleGranted { holder, .. }
        | m::ChangeEvent::DeliverableRoleGranted { holder, .. } => {
            recipients.insert(holder);
        }
        m::ChangeEvent::Posted => {
            let m::ItemRef::Post(id) = &change.item else {
                return Err(corrupt("changes.item_kind"));
            };
            let post = rows
                .posts
                .iter()
                .find(|post| &post.post_id == id)
                .ok_or_else(|| corrupt("changes.item_id"))?;
            for addressee in &post.addressees {
                match addressee {
                    m::Addressee::Actor { actor_identity_id } => {
                        recipients.insert(actor_identity_id.clone());
                    }
                    m::Addressee::ProjectRole { project_id, role } => {
                        recipients.extend(
                            rows.project_roles
                                .iter()
                                .filter(|assignment| {
                                    &assignment.project_id == project_id
                                        && &assignment.role == role
                                        && assignment.state == m::AssignmentState::Active
                                })
                                .map(|assignment| assignment.holder.clone()),
                        );
                    }
                    m::Addressee::DeliverableRole {
                        deliverable_id,
                        role,
                    } => {
                        recipients.extend(
                            rows.deliverable_roles
                                .iter()
                                .filter(|assignment| {
                                    &assignment.deliverable_id == deliverable_id
                                        && &assignment.role == role
                                        && assignment.state == m::AssignmentState::Active
                                })
                                .map(|assignment| assignment.holder.clone()),
                        );
                    }
                }
            }
        }
        _ => {}
    }
    Ok(recipients)
}
