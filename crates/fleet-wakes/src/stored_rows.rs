use central_storage::{ReadOutcome, StoreFailure};
use fleet_engine::guarded_attempt_sql;
use fleet_runtime_ports::{
    stored_values::{json, string},
    workspace_decode_error::corrupt,
};
use workspace_ids::SessionRef;
use workspace_model::{PostText, Timestamp, central_mail as mail};

use workspace_ids::WakeId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WakeRow {
    pub(crate) id: WakeId,
    pub(crate) target: SessionRef,
    pub(crate) due: Timestamp,
    pub(crate) text: PostText,
    pub(crate) state: mail::WakeState,
}
fn parse_time(value: &str, field: &str) -> Result<Timestamp, StoreFailure> {
    let time: Timestamp = parse(value, field)?;
    if !central_engine_model::supported_time(time) {
        return Err(corrupt(field));
    }
    Ok(time)
}
fn parse<T: std::str::FromStr>(value: &str, column: &str) -> Result<T, StoreFailure> {
    value.parse().map_err(|_| corrupt(column))
}
pub(crate) fn decode_wakes(rows: &ReadOutcome) -> Result<Vec<WakeRow>, StoreFailure> {
    rows.rows
        .iter()
        .map(|row| {
            Ok(WakeRow {
                id: parse(string(rows, row, "wake_id")?, "automation_wakes.wake_id")?,
                target: json(string(rows, row, "target")?, "automation_wakes.target")?,
                due: {
                    let due = parse_time(string(rows, row, "due_at")?, "automation_wakes.due_at")?;
                    let created_at = parse_time(
                        string(rows, row, "created_at")?,
                        "automation_wakes.created_at",
                    )?;
                    let expiry = fleet_timer_policy::lifetime_end(created_at)
                        .ok_or_else(|| corrupt("automation_wakes.created_at"))?;
                    if !central_engine_model::on_five_minute_grid(due)
                        || due <= created_at
                        || due > expiry
                    {
                        return Err(corrupt("automation_wakes.due_at"));
                    }
                    due
                },
                text: string(rows, row, "text")?
                    .to_owned()
                    .try_into()
                    .map_err(|_| corrupt("automation_wakes.text"))?,
                state: serde_json::from_value(serde_json::Value::String(
                    string(rows, row, "state")?.to_owned(),
                ))
                .map_err(|_| corrupt("automation_wakes.state"))?,
            })
        })
        .collect()
}

pub(crate) const WAKE_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM automation_wakes WHERE wake_id IS ?20 AND target IS ?21 AND due_at IS ?22 AND text IS ?23 AND state IS ?24 AND created_at IS ?25 AND updated_at IS ?26 AND state NOT IN ('done','disabled'))"
);
