//! Position-ordered notice decisions from a single workspace snapshot.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::timestamp,
    guarded_statements::{any, text},
    notification_windows::{NotificationWindow, delete_window, notice_item, window_statements},
    workspace_decode_error::corrupt,
    workspace_row_codec::WorkspaceRows,
};
use central_storage::{GuardedStatement, StoreFailure};
use collaboration_protocol::central_mail as mail;
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
                .read(crate::central_mail_records::workspace_messages(workspace))
                .await?,
        )?;
        let mut windows: Vec<_> = self
            .notification_windows()
            .await?
            .into_iter()
            .filter(|window| &window.workspace == workspace)
            .collect();
        let mut statements = vec![];
        let mut result = DrainResult {
            through: rows.high_water,
            ..Default::default()
        };
        let mut changes: Vec<_> = rows.changes.iter().collect();
        changes.sort_by_key(|change| change.position);
        for change in changes {
            let event = change.change()?.event;
            if let Some(thread) = &change.thread_id {
                let cancel_member = match &event {
                    m::ChangeEvent::Left { member }
                    | m::ChangeEvent::NotificationsOff { member } => Some(member),
                    _ => None,
                };
                let cancel_all = matches!(event, m::ChangeEvent::Resolved);
                if cancel_member.is_some() || cancel_all {
                    let mut retained = vec![];
                    for window in windows.drain(..) {
                        if &window.thread == thread
                            && (cancel_all || cancel_member == Some(&window.recipient))
                        {
                            statements.extend(delete_window(&window.id));
                        } else {
                            retained.push(window);
                        }
                    }
                    windows = retained;
                    statements.push(any("UPDATE messages SET state='cancelled',updated_at=? WHERE source_workspace_id=? AND source_thread_id=? AND state='queued' AND message_kind='notice' AND (? IS NULL OR recipient_actor=?)",vec![text(timestamp(now)),text(workspace),text(thread),cancel_member.map_or(central_storage::SqlValue::Null,text),cancel_member.map_or(central_storage::SqlValue::Null,text)]));
                    continue;
                }
            }
            let immediate = immediate_recipients(change, rows)?;
            for recipient in &immediate {
                if let Some(target) = self.route_notice_actor(recipient).await? {
                    let message = self.queued_message(
                        mail::MessageSender::Actor(change.actor.clone()),
                        target,
                        mail::MessageBody::Notice {
                            items: vec![notice_item(change)?],
                        },
                        Some(mail::NoticeSource {
                            change_id: change.change_id.clone(),
                            recipient_actor: recipient.clone(),
                            workspace_id: workspace.clone(),
                            thread_id: change.thread_id.clone(),
                        }),
                        now,
                    )?;
                    statements.push(crate::central_mail_store::insert_message(&message)?);
                    result.notices_queued += 1;
                } else {
                    result.skipped_recipients += 1;
                }
            }
            let Some(thread) = &change.thread_id else {
                continue;
            };
            let Some(current_thread) = rows.threads.iter().find(|row| &row.thread_id == thread)
            else {
                return Err(corrupt("changes.thread_id"));
            };
            // A terminal thread has no live subscriber candidates; the ordered resolution
            // event still cancels notices accumulated earlier in this drain.
            if current_thread.state == m::ThreadState::Resolved {
                continue;
            }
            for member in rows.members.iter().filter(|member| {
                &member.thread_id == thread
                    && member.state == m::MembershipState::Joined
                    && member.notifications == m::NotificationSetting::On
                    && member.actor_identity_id != change.actor
                    && !immediate.contains(&member.actor_identity_id)
            }) {
                let recipient = &member.actor_identity_id;
                if self.route_notice_actor(recipient).await?.is_none() {
                    result.skipped_recipients += 1;
                    continue;
                }
                let window = if let Some(window) = windows
                    .iter()
                    .find(|window| &window.thread == thread && &window.recipient == recipient)
                {
                    window.clone()
                } else {
                    let first = change
                        .created_at
                        .ok_or_else(|| corrupt("changes.created_at"))?;
                    let due = first
                        .checked_add_signed(chrono::Duration::seconds(i64::from(
                            self.mail_policy.window_seconds(),
                        )))
                        .ok_or_else(|| corrupt("notification_windows.due_at"))?;
                    if !crate::central_mail_time::supported(due) {
                        return Err(corrupt("notification_windows.due_at"));
                    }
                    let window = NotificationWindow {
                        id: self.mint()?,
                        recipient: recipient.clone(),
                        workspace: workspace.clone(),
                        thread: thread.clone(),
                        first,
                        due,
                    };
                    windows.push(window.clone());
                    window
                };
                statements.extend(window_statements(
                    &window,
                    &change.change_id,
                    self.mint()?,
                    now,
                ));
                result.window_items_added += 1;
            }
        }
        Ok((statements, result))
    }
}
fn immediate_recipients(
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
