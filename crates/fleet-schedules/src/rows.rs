use central_storage::{ReadOutcome, StoreFailure};
use fleet_engine::guarded_attempt_sql;
use fleet_runtime_ports::{
    stored_values::{json, string},
    workspace_decode_error::corrupt,
};
use workspace_ids::SessionRef;
use workspace_model::{PostText, Timestamp, central_mail as mail};

use agent_automation::TimingRule;
use workspace_ids::AutomationId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationRow {
    pub(crate) id: AutomationId,
    pub(crate) target: SessionRef,
    pub(crate) timing: TimingRule,
    pub(crate) anchor: Timestamp,
    pub(crate) expires_at: Timestamp,
    pub(crate) evaluated: Option<Timestamp>,
    pub(crate) due: Option<Timestamp>,
    pub(crate) text: PostText,
    pub(crate) state: mail::AutomationState,
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
            .map_err(|_| corrupt("automation_schedules.state"))?;
            let due = optional_time(rows, row, "next_due_at")?;
            let evaluated = optional_time(rows, row, "evaluated_through_at")?;
            if state == mail::AutomationState::Active && due.is_none() {
                return Err(corrupt("automation_schedules.next_due_at"));
            }
            if state == mail::AutomationState::Done && due.is_some() {
                return Err(corrupt("automation_schedules.next_due_at"));
            }
            if due
                .zip(evaluated)
                .is_some_and(|(due, evaluated)| due <= evaluated)
            {
                return Err(corrupt("automation_schedules.next_due_at"));
            }
            let timing: TimingRule = json(
                string(rows, row, "timing_rule")?,
                "automation_schedules.timing_rule",
            )?;
            let anchor = parse_time(
                string(rows, row, "anchor_at")?,
                "automation_schedules.anchor_at",
            )?;
            let expires_at = parse_time(
                string(rows, row, "expires_at")?,
                "automation_schedules.expires_at",
            )?;
            let admission = parse_time(
                string(rows, row, "created_at")?,
                "automation_schedules.created_at",
            )?;
            let expected_expiry = fleet_timer_policy::lifetime_end(admission)
                .ok_or_else(|| corrupt("automation_schedules.created_at"))?;
            if expires_at != expected_expiry {
                return Err(corrupt("automation_schedules.expires_at"));
            }
            if central_engine_model::aligned_at_or_after(admission, 300) != Some(anchor) {
                return Err(corrupt("automation_schedules.anchor_at"));
            }
            if due.is_some_and(|due| {
                !central_engine_model::on_five_minute_grid(due) || due > expires_at || due < anchor
            }) {
                return Err(corrupt("automation_schedules.next_due_at"));
            }
            if state == mail::AutomationState::Active {
                let due = due.ok_or_else(|| corrupt("automation_schedules.next_due_at"))?;
                let matches_rule = match &timing {
                    TimingRule::At { at } => *at > admission && due == *at,
                    TimingRule::After { seconds } => {
                        *seconds >= 300
                            && *seconds <= 604_800
                            && *seconds % 300 == 0
                            && crate::central_mail_time::first_due(&timing, admission)
                                .map_err(|_| corrupt("automation_schedules.timing_rule"))?
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
                            .ok_or_else(|| corrupt("automation_schedules.next_due_at"))?;
                        timing
                            .next_due(admission, Some(before))
                            .map_err(|_| corrupt("automation_schedules.timing_rule"))?
                            == Some(due)
                    }
                };
                if !matches_rule {
                    return Err(corrupt("automation_schedules.next_due_at"));
                }
            }
            Ok(AutomationRow {
                id: parse(
                    string(rows, row, "automation_id")?,
                    "automation_schedules.automation_id",
                )?,
                target: json(string(rows, row, "target")?, "automation_schedules.target")?,
                timing,
                anchor,
                expires_at,
                evaluated,
                due,
                text: string(rows, row, "text")?
                    .to_owned()
                    .try_into()
                    .map_err(|_| corrupt("automation_schedules.text"))?,
                state,
            })
        })
        .collect()
}

pub(crate) const AUTOMATION_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM automation_schedules WHERE automation_id IS ?20 AND target IS ?21 AND timing_rule IS ?22 AND anchor_at IS ?23 AND expires_at IS ?24 AND evaluated_through_at IS ?25 AND text IS ?26 AND next_due_at IS ?27 AND state IS ?28 AND created_at IS ?29 AND updated_at IS ?30 AND state NOT IN ('done','disabled'))"
);

pub(crate) fn timestamp(value: Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
