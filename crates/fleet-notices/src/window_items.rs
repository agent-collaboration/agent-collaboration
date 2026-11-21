//! Reload the source changes attached to a notice window.
use crate::window_rows::parse;
use central_storage::{SqlValue, Statement, StoreFailure};
use fleet_contracts::LegacyUnitOutcome;
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{
    CentralStore, WorkspaceStores,
    guarded_statements::text,
    stored_values::{column, encode_json, json, string},
    workspace_decode_error::corrupt,
};
use fleet_workspace_access::workspace_change_records::StoredChange;
use workspace_ids::{NotificationWindowId, NotificationWindowItemId};
use workspace_model::{ChangeId, ThreadId, WorkspaceId, central_mail as mail};

pub(crate) async fn window_notice_items<Host: CentralStore, Stores: WorkspaceStores>(
    execution: &FleetRuntime<Host, Stores>,
    window: &NotificationWindowId,
) -> Result<Vec<mail::NoticeItem>, LegacyUnitOutcome> {
    let rows = execution
        .statement_host()
        .read(Statement {
            sql: "SELECT window_item_id,workspace_id,thread_id,change_id FROM mail_notification_window_items WHERE window_id=? ORDER BY workspace_id,change_id",
            args: vec![text(window)]
        })
        .await?;
    let mut grouped: std::collections::BTreeMap<WorkspaceId, Vec<(ChangeId, Option<ThreadId>)>> =
        Default::default();
    for row in &rows.rows {
        let _: NotificationWindowItemId = parse(
            string(&rows, row, "window_item_id")?,
            "mail_notification_window_items.window_item_id",
        )?;
        let workspace = parse(
            string(&rows, row, "workspace_id")?,
            "mail_notification_window_items.workspace_id",
        )?;
        let thread = match column(&rows, row, "thread_id")? {
            SqlValue::Null => None,
            SqlValue::Text(value) => {
                Some(parse(value, "mail_notification_window_items.thread_id")?)
            }
            _ => return Err(corrupt("mail_notification_window_items.thread_id").into()),
        };
        let change = parse(
            string(&rows, row, "change_id")?,
            "mail_notification_window_items.change_id",
        )?;
        grouped.entry(workspace).or_default().push((change, thread));
    }
    if grouped.is_empty() {
        return Err(corrupt("mail_notification_window_items").into());
    }
    let mut items = Vec::new();
    for (workspace, changes) in grouped {
        let ids: Vec<_> = changes.iter().map(|(id, _)| id).collect();
        let encoded = encode_json(&ids, "mail JSON")?;
        let rows = execution
            .read_workspace(&workspace, Statement {
                sql: "SELECT json_object('changeId',change_id,'position',position,'item',json_object('kind',item_kind,'id',item_id),'actor',actor,'changeKind',change_kind,'threadId',thread_id,'projectId',project_id,'subjectActor',subject_actor,'role',role,'createdAt',created_at) AS payload FROM workspace_changes WHERE change_id IN (SELECT value FROM json_each(?)) ORDER BY position",
                args: vec![encoded]
            })
            .await
            .map_err(|failure| match failure {
                StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. } => LegacyUnitOutcome::RemoteUnavailable {
                    workspaces: vec![workspace.clone()]
                },
                failure @ (StoreFailure::Corrupt { .. } | StoreFailure::AnswerLost) => failure.into()
            })?;
        if rows.rows.len() != changes.len() {
            return Err(corrupt("mail_notification_window_items.change_id").into());
        }
        for row in &rows.rows {
            let stored: StoredChange = json(string(&rows, row, "payload")?, "workspace_changes")?;
            if !changes
                .iter()
                .any(|(id, thread)| id == &stored.change_id && thread == &stored.thread_id)
            {
                return Err(corrupt("mail_notification_window_items.change_id").into());
            }
            items.push(notice_item(&stored, &workspace)?);
        }
    }
    Ok(items)
}

fn notice_item(
    stored: &StoredChange,
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
