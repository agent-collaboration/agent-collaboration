//! Recipient windows collate source-owned activity across workspace boundaries.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{self, timestamp},
    guarded_statements::{any, exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, json, string},
};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use collaboration_protocol::central_mail as mail;
use workspace_ids::{NotificationWindowId, NotificationWindowItemId};
use workspace_model::{
    ActorIdentityId, ChangeId, ChangePosition, NoticeCadence, ThreadId, Timestamp, WorkspaceId,
};

#[derive(Clone)]
pub(crate) struct NotificationWindow {
    pub id: NotificationWindowId,
    pub recipient: ActorIdentityId,
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
    pub(crate) async fn notice_due(
        &self,
        recipient: &ActorIdentityId,
        first: Timestamp,
    ) -> Result<Timestamp, StoreFailure> {
        let rows = self.host.read(Statement { sql: "SELECT notice_cadence,last_notice_delivery_at FROM actor_identities WHERE actor_identity_id=?", args: vec![text(recipient)] }).await?;
        let row = rows
            .rows
            .first()
            .ok_or_else(|| corrupt("notification_windows.recipient_actor"))?;
        let cadence: NoticeCadence = serde_json::from_value(serde_json::Value::String(
            string(&rows, row, "notice_cadence")?.to_owned(),
        ))
        .map_err(|_| corrupt("actor_identities.notice_cadence"))?;
        self.notice_due_with_cadence(recipient, first, cadence)
            .await
    }
    pub(crate) async fn notice_due_with_cadence(
        &self,
        recipient: &ActorIdentityId,
        first: Timestamp,
        cadence: NoticeCadence,
    ) -> Result<Timestamp, StoreFailure> {
        let rows = self.host.read(Statement { sql: "SELECT last_notice_delivery_at FROM actor_identities WHERE actor_identity_id=?", args: vec![text(recipient)] }).await?;
        let row = rows
            .rows
            .first()
            .ok_or_else(|| corrupt("notification_windows.recipient_actor"))?;
        let last = match column(&rows, row, "last_notice_delivery_at")? {
            SqlValue::Null => first,
            SqlValue::Text(value) => {
                let last = parse_time(value, "actor_identities.last_notice_delivery_at")?;
                if !crate::central_mail_time::on_grid(last) {
                    return Err(corrupt("actor_identities.last_notice_delivery_at"));
                }
                last
            }
            _ => return Err(corrupt("actor_identities.last_notice_delivery_at")),
        };
        last.checked_add_signed(chrono::Duration::seconds(i64::from(cadence.seconds())))
            .and_then(|due| crate::central_mail_time::aligned_at_or_after(due.max(first), 300))
            .filter(|due| crate::central_mail_time::supported(*due))
            .ok_or_else(|| corrupt("notification_windows.due_at"))
    }
    pub(crate) async fn notification_windows(
        &self,
    ) -> Result<Vec<NotificationWindow>, StoreFailure> {
        let rows=self.host.read(Statement{sql:"SELECT window_id,recipient_actor,first_activity_at,due_at FROM notification_windows",args:vec![]}).await?;
        let mut windows = Vec::new();
        for row in &rows.rows {
            let first = parse_time(
                string(&rows, row, "first_activity_at")?,
                "notification_windows.first_activity_at",
            )?;
            let due = parse_time(string(&rows, row, "due_at")?, "notification_windows.due_at")?;
            if due < first || !crate::central_mail_time::on_grid(due) {
                return Err(corrupt("notification_windows.due_at"));
            }
            let recipient = parse(
                string(&rows, row, "recipient_actor")?,
                "notification_windows.recipient_actor",
            )?;
            // Validate the owning actor's stored policy before using any deadline.
            self.notice_due(&recipient, first).await?;
            windows.push(NotificationWindow {
                id: parse(
                    string(&rows, row, "window_id")?,
                    "notification_windows.window_id",
                )?,
                recipient,
                first,
                due,
            });
        }
        Ok(windows)
    }
    pub(crate) async fn validate_notification_due_rows(&self) -> Result<(), StoreFailure> {
        self.notification_windows().await?;
        let rows=self.host.read(Statement{sql:"SELECT workspace_id,drained_through_position,drain_due_at FROM workspace_drains",args:vec![]}).await?;
        for row in &rows.rows {
            let _: WorkspaceId = parse(
                string(&rows, row, "workspace_id")?,
                "workspace_drains.workspace_id",
            )?;
            match column(&rows, row, "drained_through_position")? {
                SqlValue::Integer(position) => {
                    let _: ChangePosition = u64::try_from(position.get())
                        .ok()
                        .and_then(|value| ChangePosition::try_from(value).ok())
                        .ok_or_else(|| corrupt("workspace_drains.drained_through_position"))?;
                }
                _ => return Err(corrupt("workspace_drains.drained_through_position")),
            }
            match column(&rows, row, "drain_due_at")? {
                SqlValue::Null => {}
                SqlValue::Text(value) => {
                    parse_time(value, "workspace_drains.drain_due_at")?;
                }
                _ => return Err(corrupt("workspace_drains.drain_due_at")),
            }
        }
        Ok(())
    }
    async fn window_notice_items(
        &self,
        window: &NotificationWindowId,
    ) -> Result<Vec<mail::NoticeItem>, StoreFailure> {
        let rows=self.host.read(Statement{sql:"SELECT window_item_id,workspace_id,thread_id,change_id FROM notification_window_items WHERE window_id=? ORDER BY workspace_id,change_id",args:vec![text(window)]}).await?;
        let mut grouped: std::collections::BTreeMap<
            WorkspaceId,
            Vec<(ChangeId, Option<ThreadId>)>,
        > = Default::default();
        for row in &rows.rows {
            let _: NotificationWindowItemId = parse(
                string(&rows, row, "window_item_id")?,
                "notification_window_items.window_item_id",
            )?;
            let workspace = parse(
                string(&rows, row, "workspace_id")?,
                "notification_window_items.workspace_id",
            )?;
            let thread = match column(&rows, row, "thread_id")? {
                SqlValue::Null => None,
                SqlValue::Text(value) => Some(parse(value, "notification_window_items.thread_id")?),
                _ => return Err(corrupt("notification_window_items.thread_id")),
            };
            let change = parse(
                string(&rows, row, "change_id")?,
                "notification_window_items.change_id",
            )?;
            grouped.entry(workspace).or_default().push((change, thread));
        }
        if grouped.is_empty() {
            return Err(corrupt("notification_window_items"));
        }
        let mut items = Vec::new();
        for (workspace, changes) in grouped {
            let ids: Vec<_> = changes.iter().map(|(id, _)| id).collect();
            let rows=self.read_workspace(&workspace,Statement{sql:"SELECT json_object('changeId',change_id,'position',position,'item',json_object('kind',item_kind,'id',item_id),'actor',actor,'changeKind',change_kind,'threadId',thread_id,'projectId',project_id,'subjectActor',subject_actor,'role',role,'createdAt',created_at) AS payload FROM changes WHERE change_id IN (SELECT value FROM json_each(?)) ORDER BY position",args:vec![central_mail_store::encode(&ids)?]}).await?;
            if rows.rows.len() != changes.len() {
                return Err(corrupt("notification_window_items.change_id"));
            }
            for row in &rows.rows {
                let stored: crate::workspace_change_records::StoredChange =
                    json(string(&rows, row, "payload")?, "changes")?;
                if !changes
                    .iter()
                    .any(|(id, thread)| id == &stored.change_id && thread == &stored.thread_id)
                {
                    return Err(corrupt("notification_window_items.change_id"));
                }
                items.push(notice_item(&stored, &workspace)?);
            }
        }
        Ok(items)
    }
    pub(crate) async fn plan_due_windows(
        &self,
        now: Timestamp,
    ) -> Result<(Vec<GuardedStatement>, usize), StoreFailure> {
        let mut statements = Vec::new();
        let mut count = 0;
        for window in self
            .notification_windows()
            .await?
            .into_iter()
            .filter(|window| window.due <= now)
        {
            let items = self.window_notice_items(&window.id).await?;
            if let Some(target) = self.route_notice_actor(&window.recipient).await? {
                let first = items
                    .first()
                    .ok_or_else(|| corrupt("notification_window_items"))?;
                let source = mail::NoticeSource {
                    change_id: first.change_id.clone(),
                    recipient_actor: window.recipient.clone(),
                    workspace_id: first.workspace_id.clone(),
                    thread_id: first.thread_id.clone(),
                };
                let mut message = self.prepare_message_record(
                    mail::MessageSender::Central,
                    target,
                    mail::MessageBody::Notice { items },
                    Some(source),
                    now,
                )?;
                message.deliver_after = window.due;
                statements.push(central_mail_store::insert_message(&message)?);
                statements.push(exact("UPDATE actor_identities SET last_notice_delivery_at=?,updated_at=? WHERE actor_identity_id=?",vec![text(timestamp(window.due)),text(timestamp(now)),text(&window.recipient)]));
                count += 1;
            }
            statements.extend(delete_window(&window.id));
        }
        Ok((statements, count))
    }
    pub(crate) async fn cancel_ready_notices(
        &self,
        workspace: &WorkspaceId,
        thread: &ThreadId,
        recipient: Option<&ActorIdentityId>,
        now: Timestamp,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        let rows = self
            .host
            .read(crate::central_mail_records::notice_messages())
            .await?;
        crate::central_mail_records::decode_messages(&rows)?;
        let recipient = recipient.map_or(SqlValue::Null, text);
        // Apply each cancellation against the transaction's current body. Two source
        // cancellations in one drain must never restore each other's removed items.
        Ok(vec![
            any(
                "UPDATE messages SET body=json_set(body,'$.items',json(COALESCE((SELECT json_group_array(json(value)) FROM json_each(json_extract(messages.body,'$.items')) WHERE json_extract(value,'$.workspaceId')!=? OR json_extract(value,'$.threadId') IS NOT ?),'[]'))),updated_at=? WHERE message_kind='notice' AND state IN ('queued','ready') AND (? IS NULL OR recipient_actor=?)",
                vec![
                    text(workspace),
                    text(thread),
                    text(timestamp(now)),
                    recipient.clone(),
                    recipient,
                ],
            ),
            any(
                "UPDATE messages SET state=CASE WHEN json_array_length(body,'$.items')=0 THEN 'cancelled' ELSE state END,source_change_id=COALESCE(json_extract(body,'$.items[0].changeId'),source_change_id),source_workspace_id=COALESCE(json_extract(body,'$.items[0].workspaceId'),source_workspace_id),source_thread_id=CASE WHEN json_array_length(body,'$.items')=0 THEN source_thread_id ELSE json_extract(body,'$.items[0].threadId') END WHERE message_kind='notice' AND state IN ('queued','ready')",
                vec![],
            ),
        ])
    }
}
pub(crate) fn notice_item(
    stored: &crate::workspace_change_records::StoredChange,
    workspace: &WorkspaceId,
) -> Result<mail::NoticeItem, StoreFailure> {
    let change = stored.change()?;
    Ok(mail::NoticeItem {
        workspace_id: workspace.clone(),
        thread_id: stored.thread_id.clone(),
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
    workspace: &WorkspaceId,
    thread: Option<&ThreadId>,
    change: &ChangeId,
    item: NotificationWindowItemId,
    now: Timestamp,
) -> Vec<GuardedStatement> {
    vec![
        any(
            "INSERT INTO notification_windows(window_id,recipient_actor,first_activity_at,due_at,created_at,updated_at) VALUES (?,?,?,?,?,?) ON CONFLICT(recipient_actor) DO NOTHING",
            vec![
                text(&window.id),
                text(&window.recipient),
                text(timestamp(window.first)),
                text(timestamp(window.due)),
                text(timestamp(now)),
                text(timestamp(now)),
            ],
        ),
        any(
            "INSERT INTO notification_window_items(window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?) ON CONFLICT(window_id,workspace_id,change_id) DO NOTHING",
            vec![
                text(item),
                text(&window.id),
                text(workspace),
                thread.map_or(SqlValue::Null, text),
                text(change),
                text(timestamp(now)),
                text(timestamp(now)),
            ],
        ),
    ]
}
