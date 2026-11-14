//! Fixed notification windows and immutable change collation, owned by central.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{self, timestamp},
    guarded_statements::{any, exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use collaboration_protocol::central_mail as mail;
use workspace_ids::{NotificationWindowId, NotificationWindowItemId};
use workspace_model::{
    ActorIdentityId, ChangeId, ChangePosition, ThreadId, Timestamp, WorkspaceId,
};

#[derive(Clone)]
pub(crate) struct NotificationWindow {
    pub id: NotificationWindowId,
    pub recipient: ActorIdentityId,
    pub workspace: WorkspaceId,
    pub thread: ThreadId,
    pub first: Timestamp,
    pub due: Timestamp,
}
pub(crate) fn parse<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, StoreFailure> {
    value.parse().map_err(|_| corrupt(field))
}
pub(crate) fn parse_time(value: &str, field: &str) -> Result<Timestamp, StoreFailure> {
    let time: Timestamp = parse(value, field)?;
    if !crate::central_mail_time::supported(time) {
        return Err(corrupt(field));
    }
    Ok(time)
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn notification_windows(
        &self,
    ) -> Result<Vec<NotificationWindow>, StoreFailure> {
        let rows=self.host.read(Statement{sql:"SELECT window_id,recipient_actor,workspace_id,thread_id,first_activity_at,due_at FROM notification_windows",args:vec![]}).await?;
        rows.rows
            .iter()
            .map(|row| {
                let first = parse_time(
                    string(&rows, row, "first_activity_at")?,
                    "notification_windows.first_activity_at",
                )?;
                let due = parse_time(string(&rows, row, "due_at")?, "notification_windows.due_at")?;
                if due < first {
                    return Err(corrupt("notification_windows.due_at"));
                }
                Ok(NotificationWindow {
                    id: parse(
                        string(&rows, row, "window_id")?,
                        "notification_windows.window_id",
                    )?,
                    recipient: parse(
                        string(&rows, row, "recipient_actor")?,
                        "notification_windows.recipient_actor",
                    )?,
                    workspace: parse(
                        string(&rows, row, "workspace_id")?,
                        "notification_windows.workspace_id",
                    )?,
                    thread: parse(
                        string(&rows, row, "thread_id")?,
                        "notification_windows.thread_id",
                    )?,
                    first,
                    due,
                })
            })
            .collect()
    }
    pub(crate) async fn validate_notification_due_rows(&self) -> Result<(), StoreFailure> {
        self.notification_windows().await?;
        let rows=self.host.read(Statement{sql:"SELECT workspace_id,drained_through_position,drain_due_at FROM workspace_drains",args:vec![]}).await?;
        for row in &rows.rows {
            let _: WorkspaceId = parse(
                string(&rows, row, "workspace_id")?,
                "workspace_drains.workspace_id",
            )?;
            match crate::workspace_row_codec::column(&rows, row, "drained_through_position")? {
                SqlValue::Integer(position) => {
                    let _: ChangePosition = u64::try_from(position.get())
                        .ok()
                        .and_then(|value| ChangePosition::try_from(value).ok())
                        .ok_or_else(|| corrupt("workspace_drains.drained_through_position"))?;
                }
                _ => return Err(corrupt("workspace_drains.drained_through_position")),
            }
            match crate::workspace_row_codec::column(&rows, row, "drain_due_at")? {
                SqlValue::Null => {}
                SqlValue::Text(value) => {
                    let _: Timestamp = parse_time(value, "workspace_drains.drain_due_at")?;
                }
                _ => return Err(corrupt("workspace_drains.drain_due_at")),
            }
        }
        Ok(())
    }
    pub(crate) async fn plan_due_windows(
        &self,
        now: Timestamp,
    ) -> Result<(Vec<GuardedStatement>, usize), StoreFailure> {
        let mut statements = vec![];
        let mut count = 0;
        for window in self
            .notification_windows()
            .await?
            .into_iter()
            .filter(|window| window.due <= now)
        {
            let rows=self.host.read(Statement{sql:"SELECT window_item_id,change_id FROM notification_window_items WHERE window_id=?",args:vec![text(&window.id)]}).await?;
            let changes: Vec<ChangeId> = rows
                .rows
                .iter()
                .map(|row| {
                    let _: NotificationWindowItemId = parse(
                        string(&rows, row, "window_item_id")?,
                        "notification_window_items.window_item_id",
                    )?;
                    parse(
                        string(&rows, row, "change_id")?,
                        "notification_window_items.change_id",
                    )
                })
                .collect::<Result<_, StoreFailure>>()?;
            if changes.is_empty() {
                return Err(corrupt("notification_window_items"));
            }
            let rows=self.read_workspace(&window.workspace,Statement{sql:"SELECT json_object('changeId',change_id,'position',position,'item',json_object('kind',item_kind,'id',item_id),'actor',actor,'changeKind',change_kind,'threadId',thread_id,'projectId',project_id,'subjectActor',subject_actor,'role',role,'createdAt',created_at) AS payload FROM changes WHERE change_id IN (SELECT value FROM json_each(?)) ORDER BY position",args:vec![central_mail_store::encode(&changes)?]}).await?;
            let items = rows
                .rows
                .iter()
                .map(|row| {
                    let stored: crate::workspace_change_records::StoredChange =
                        json(string(&rows, row, "payload")?, "changes")?;
                    if stored.thread_id.as_ref() != Some(&window.thread) {
                        return Err(corrupt("notification_window_items.change_id"));
                    }
                    notice_item(&stored)
                })
                .collect::<Result<Vec<_>, StoreFailure>>()?;
            if items.len() != changes.len() {
                return Err(corrupt("notification_window_items.change_id"));
            }
            if let Some(target) = self.route_notice_actor(&window.recipient).await? {
                let source = mail::NoticeSource {
                    change_id: items
                        .first()
                        .ok_or_else(|| corrupt("notification_window_items"))?
                        .change_id
                        .clone(),
                    recipient_actor: window.recipient.clone(),
                    workspace_id: window.workspace.clone(),
                    thread_id: Some(window.thread.clone()),
                };
                let message = self.queued_message(
                    mail::MessageSender::Central,
                    target,
                    mail::MessageBody::Notice { items },
                    Some(source),
                    now,
                )?;
                statements.push(central_mail_store::insert_message(&message)?);
                count += 1;
            }
            statements.extend(delete_window(&window.id));
        }
        Ok((statements, count))
    }
}
pub(crate) fn notice_item(
    stored: &crate::workspace_change_records::StoredChange,
) -> Result<mail::NoticeItem, StoreFailure> {
    let change = stored.change()?;
    Ok(mail::NoticeItem {
        change_id: change.change_id,
        position: change.position,
        item: change.item,
        actor: change.actor,
        event: change.event,
    })
}
pub(crate) fn delete_window(id: &NotificationWindowId) -> Vec<GuardedStatement> {
    vec![
        any(
            "DELETE FROM notification_window_items WHERE window_id=?",
            vec![text(id)],
        ),
        exact(
            "DELETE FROM notification_windows WHERE window_id=?",
            vec![text(id)],
        ),
    ]
}
pub(crate) fn window_statements(
    window: &NotificationWindow,
    change: &ChangeId,
    item: NotificationWindowItemId,
    now: Timestamp,
) -> Vec<GuardedStatement> {
    vec![
        any(
            "INSERT INTO notification_windows(window_id,recipient_actor,workspace_id,thread_id,first_activity_at,due_at,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(recipient_actor,workspace_id,thread_id) DO NOTHING",
            vec![
                text(&window.id),
                text(&window.recipient),
                text(&window.workspace),
                text(&window.thread),
                text(timestamp(window.first)),
                text(timestamp(window.due)),
                text(timestamp(now)),
                text(timestamp(now)),
            ],
        ),
        any(
            "INSERT INTO notification_window_items(window_item_id,window_id,change_id,created_at,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(window_id,change_id) DO NOTHING",
            vec![
                text(item),
                text(&window.id),
                text(change),
                text(timestamp(now)),
                text(timestamp(now)),
            ],
        ),
    ]
}
