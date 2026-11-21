//! Plan writes and removals of notice windows and their attached changes.
use crate::window_rows::NotificationWindow;
use central_storage::{GuardedStatement, SqlValue};
use fleet_runtime_ports::guarded_statements::{any, exact, text};
use fleet_runtime_ports::stored_values::format_utc_millis;
use workspace_ids::{NotificationWindowId, NotificationWindowItemId};
use workspace_model::{ChangeId, ThreadId, Timestamp, WorkspaceId};

pub(crate) fn delete_window_items_for_thread(
    workspace: &WorkspaceId,
    thread: &ThreadId,
    recipient: Option<&workspace_model::ActorIdentityId>,
) -> GuardedStatement {
    any(
        "DELETE FROM mail_notification_window_items WHERE workspace_id=? AND thread_id=? AND (? IS NULL OR window_id IN (SELECT window_id FROM mail_notification_windows WHERE recipient_actor=?))",
        vec![
            text(workspace),
            text(thread),
            recipient.map_or(SqlValue::Null, text),
            recipient.map_or(SqlValue::Null, text),
        ],
    )
}

pub(crate) fn delete_empty_windows() -> GuardedStatement {
    any(
        "DELETE FROM mail_notification_windows WHERE NOT EXISTS(SELECT 1 FROM mail_notification_window_items i WHERE i.window_id=mail_notification_windows.window_id)",
        vec![],
    )
}

pub(crate) fn delete_window(id: &NotificationWindowId) -> Vec<GuardedStatement> {
    vec![
        any(
            "DELETE FROM mail_notification_window_items WHERE window_id=?",
            vec![text(id)],
        ),
        exact(
            "DELETE FROM mail_notification_windows WHERE window_id=?",
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
            "INSERT INTO mail_notification_windows(window_id,recipient_actor,first_activity_at,due_at,created_at,updated_at) VALUES (?,?,?,?,?,?) ON CONFLICT(recipient_actor) DO NOTHING",
            vec![
                text(&window.id),
                text(&window.recipient),
                text(format_utc_millis(window.first)),
                text(format_utc_millis(window.due)),
                text(format_utc_millis(now)),
                text(format_utc_millis(now)),
            ],
        ),
        any(
            "INSERT INTO mail_notification_window_items(window_item_id,window_id,workspace_id,thread_id,change_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?) ON CONFLICT(window_id,workspace_id,change_id) DO NOTHING",
            vec![
                text(item),
                text(&window.id),
                text(workspace),
                thread.map_or(SqlValue::Null, text),
                text(change),
                text(format_utc_millis(now)),
                text(format_utc_millis(now)),
            ],
        ),
    ]
}
