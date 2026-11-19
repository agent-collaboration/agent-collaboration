//! Validated wake and automation rows, decoded without any runtime, so request
//! plans and the engine machines share them with no module cycle.
use agent_automation::TimingRule;
use central_storage::{ReadOutcome, StoreFailure};
use fleet_runtime_ports::stored_values::json;
use fleet_runtime_ports::stored_values::string;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use workspace_ids::{AutomationId, WakeId};
use workspace_model::central_mail as mail;
use workspace_model::{PostText, SessionAddress, Timestamp};
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WakeRow {
    pub id: WakeId,
    pub target: SessionAddress,
    pub due: Timestamp,
    pub text: PostText,
    pub state: mail::WakeState,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutomationRow {
    pub id: AutomationId,
    pub target: SessionAddress,
    pub timing: TimingRule,
    pub anchor: Timestamp,
    pub expires_at: Timestamp,
    pub evaluated: Option<Timestamp>,
    pub due: Option<Timestamp>,
    pub text: PostText,
    pub state: mail::AutomationState,
}
fn parse_time(value: &str, field: &str) -> Result<Timestamp, StoreFailure> {
    let time: Timestamp = parse(value, field)?;
    if !crate::central_mail_time::supported(time) {
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
                id: parse(string(rows, row, "wake_id")?, "wakes.wake_id")?,
                target: json(string(rows, row, "target")?, "wakes.target")?,
                due: {
                    let due = parse_time(string(rows, row, "due_at")?, "wakes.due_at")?;
                    let created_at =
                        parse_time(string(rows, row, "created_at")?, "wakes.created_at")?;
                    let expiry = crate::central_mail_time::lifetime_end(created_at)
                        .ok_or_else(|| corrupt("wakes.created_at"))?;
                    if !crate::central_mail_time::on_grid(due) || due <= created_at || due > expiry
                    {
                        return Err(corrupt("wakes.due_at"));
                    }
                    due
                },
                text: string(rows, row, "text")?
                    .to_owned()
                    .try_into()
                    .map_err(|_| corrupt("wakes.text"))?,
                state: serde_json::from_value(serde_json::Value::String(
                    string(rows, row, "state")?.to_owned(),
                ))
                .map_err(|_| corrupt("wakes.state"))?,
            })
        })
        .collect()
}
fn optional_time(
    rows: &ReadOutcome,
    row: &[central_storage::SqlValue],
    column: &str,
) -> Result<Option<Timestamp>, StoreFailure> {
    match fleet_runtime_ports::stored_values::column(rows, row, column)? {
        central_storage::SqlValue::Null => Ok(None),
        central_storage::SqlValue::Text(value) => parse_time(value, column).map(Some),
        _ => Err(corrupt(column)),
    }
}
pub(crate) fn decode_automations(rows: &ReadOutcome) -> Result<Vec<AutomationRow>, StoreFailure> {
    rows.rows
        .iter()
        .map(|row| {
            let state: mail::AutomationState = serde_json::from_value(serde_json::Value::String(
                string(rows, row, "state")?.to_owned(),
            ))
            .map_err(|_| corrupt("automations.state"))?;
            let due = optional_time(rows, row, "next_due_at")?;
            let evaluated = optional_time(rows, row, "evaluated_through_at")?;
            if state == mail::AutomationState::Active && due.is_none() {
                return Err(corrupt("automations.next_due_at"));
            }
            if state == mail::AutomationState::Done && due.is_some() {
                return Err(corrupt("automations.next_due_at"));
            }
            if due
                .zip(evaluated)
                .is_some_and(|(due, evaluated)| due <= evaluated)
            {
                return Err(corrupt("automations.next_due_at"));
            }
            let timing: TimingRule =
                json(string(rows, row, "timing_rule")?, "automations.timing_rule")?;
            let anchor = parse_time(string(rows, row, "anchor_at")?, "automations.anchor_at")?;
            let expires_at =
                parse_time(string(rows, row, "expires_at")?, "automations.expires_at")?;
            let admission = parse_time(string(rows, row, "created_at")?, "automations.created_at")?;
            let expected_expiry = crate::central_mail_time::lifetime_end(admission)
                .ok_or_else(|| corrupt("automations.created_at"))?;
            if expires_at != expected_expiry {
                return Err(corrupt("automations.expires_at"));
            }
            if crate::central_mail_time::aligned_at_or_after(admission, 300) != Some(anchor) {
                return Err(corrupt("automations.anchor_at"));
            }
            if due.is_some_and(|due| {
                !crate::central_mail_time::on_grid(due) || due > expires_at || due < anchor
            }) {
                return Err(corrupt("automations.next_due_at"));
            }
            if state == mail::AutomationState::Active {
                let due = due.ok_or_else(|| corrupt("automations.next_due_at"))?;
                let matches_rule = match &timing {
                    TimingRule::At { at } => *at > admission && due == *at,
                    TimingRule::After { seconds } => {
                        *seconds >= 300
                            && *seconds <= 604_800
                            && *seconds % 300 == 0
                            && crate::central_mail_time::first_due(&timing, admission)
                                .map_err(|_| corrupt("automations.timing_rule"))?
                                == Some(due)
                    }
                    TimingRule::Interval { seconds } => {
                        *seconds >= 300
                            && *seconds <= 604_800
                            && *seconds % 300 == 0
                            && due.signed_duration_since(anchor).num_seconds() % i64::from(*seconds)
                                == 0
                    }
                    TimingRule::Cron { .. } => {
                        let before = due
                            .checked_sub_signed(chrono::Duration::seconds(1))
                            .ok_or_else(|| corrupt("automations.next_due_at"))?;
                        timing
                            .next_due(admission, Some(before))
                            .map_err(|_| corrupt("automations.timing_rule"))?
                            == Some(due)
                    }
                };
                if !matches_rule {
                    return Err(corrupt("automations.next_due_at"));
                }
            }
            Ok(AutomationRow {
                id: parse(
                    string(rows, row, "automation_id")?,
                    "automations.automation_id",
                )?,
                target: json(string(rows, row, "target")?, "automations.target")?,
                timing,
                anchor,
                expires_at,
                evaluated,
                due,
                text: string(rows, row, "text")?
                    .to_owned()
                    .try_into()
                    .map_err(|_| corrupt("automations.text"))?,
                state,
            })
        })
        .collect()
}
