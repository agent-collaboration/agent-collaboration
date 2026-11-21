//! Read and validate the stored notice windows and their cadence alignment.
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::{CentralStore, stored_values::string, workspace_decode_error::corrupt};
use fleet_workspace_access::read_notice_cadence;
use workspace_ids::NotificationWindowId;
use workspace_model::{ActorIdentityId, Timestamp};

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
    if !central_engine_model::supported_time(time) {
        return Err(corrupt(field));
    }
    Ok(time)
}

pub(crate) async fn notification_windows(
    host: &impl CentralStore,
) -> Result<Vec<NotificationWindow>, StoreFailure> {
    let rows=host.read(Statement{sql:"SELECT window_id,recipient_actor,first_activity_at,due_at FROM mail_notification_windows",args:vec![]}).await?;
    let mut windows = Vec::new();
    for row in &rows.rows {
        let first = parse_time(
            string(&rows, row, "first_activity_at")?,
            "mail_notification_windows.first_activity_at",
        )?;
        let due = parse_time(
            string(&rows, row, "due_at")?,
            "mail_notification_windows.due_at",
        )?;
        if !central_engine_model::on_five_minute_grid(due) {
            return Err(corrupt("mail_notification_windows.due_at"));
        }
        let recipient = parse(
            string(&rows, row, "recipient_actor")?,
            "mail_notification_windows.recipient_actor",
        )?;
        let cadence = read_notice_cadence(host, &recipient).await?;
        if due.timestamp().rem_euclid(i64::from(cadence.seconds())) != 0 {
            return Err(corrupt("mail_notification_windows.due_at"));
        }
        windows.push(NotificationWindow {
            id: parse(
                string(&rows, row, "window_id")?,
                "mail_notification_windows.window_id",
            )?,
            recipient,
            first,
            due,
        });
    }
    Ok(windows)
}
