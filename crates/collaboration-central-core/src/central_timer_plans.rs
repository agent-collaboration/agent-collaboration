//! Timer request plans and validated rows; occurrence calculation stays in TimingRule.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{encode, timestamp},
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use agent_automation::TimingRule;
use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use collaboration_protocol::central_mail as mail;
use workspace_ids::{AutomationId, WakeId};
use workspace_model::{PostText, SessionAddress, Timestamp};
pub(crate) struct WakeRow {
    pub id: WakeId,
    pub target: SessionAddress,
    pub due: Timestamp,
    pub text: PostText,
    pub state: mail::WakeState,
}
pub(crate) struct AutomationRow {
    pub id: AutomationId,
    pub target: SessionAddress,
    pub timing: TimingRule,
    pub anchor: Timestamp,
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
                due: parse_time(string(rows, row, "due_at")?, "wakes.due_at")?,
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
    match crate::workspace_row_codec::column(rows, row, column)? {
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
            timing
                .next_due(anchor, None)
                .map_err(|_| corrupt("automations.timing_rule"))?;
            Ok(AutomationRow {
                id: parse(
                    string(rows, row, "automation_id")?,
                    "automations.automation_id",
                )?,
                target: json(string(rows, row, "target")?, "automations.target")?,
                timing,
                anchor,
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
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn plan_timer_request(
        &self,
        operation: &mail::CentralMailOperation,
        now: Timestamp,
    ) -> Result<
        Result<(Vec<GuardedStatement>, mail::CentralMailAnswer), mail::CentralMailRefusal>,
        StoreFailure,
    > {
        use mail::{
            CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRefusal as Refusal,
        };
        let at = || text(timestamp(now));
        let decision = match operation {
            Op::SetWake {
                target,
                due_at,
                text: body,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                let id: WakeId = self.mint()?;
                (
                    vec![exact(
                        "INSERT INTO wakes(wake_id,target,due_at,text,state,created_at,updated_at) VALUES (?,?,?,?,'active',?,?)",
                        vec![
                            text(&id),
                            encode(target)?,
                            text(timestamp(*due_at)),
                            text(body.as_str()),
                            at(),
                            at(),
                        ],
                    )],
                    Answer::WakeSet {
                        wake_id: id,
                        due_at: *due_at,
                    },
                )
            }
            Op::CancelWake { wake_id } => {
                let rows = self
                    .host
                    .read(Statement {
                        sql: "SELECT wake_id,target,due_at,text,state FROM wakes WHERE wake_id=?",
                        args: vec![text(wake_id)],
                    })
                    .await?;
                let rows = decode_wakes(&rows)?;
                let Some(row) = rows.first() else {
                    return Ok(Err(Refusal::WakeNotFound {
                        wake_id: wake_id.clone(),
                    }));
                };
                if row.state == mail::WakeState::Done {
                    return Ok(Err(Refusal::WakeAlreadyFired {
                        wake_id: wake_id.clone(),
                    }));
                }
                let statements = if row.state == mail::WakeState::Disabled {
                    vec![]
                } else {
                    vec![exact(
                        "UPDATE wakes SET state='disabled',updated_at=? WHERE wake_id=? AND state='active'",
                        vec![at(), text(wake_id)],
                    )]
                };
                (
                    statements,
                    Answer::WakeCancelled {
                        wake_id: wake_id.clone(),
                    },
                )
            }
            Op::SetAutomation {
                target,
                timing,
                text: body,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                let first = match timing.next_due(now, None) {
                    Ok(Some(first)) => first,
                    Ok(None) | Err(agent_automation::TimingError::OutOfRange) => {
                        return Ok(Err(Refusal::NoOccurrence));
                    }
                    Err(_) => {
                        return Ok(Err(Refusal::InvalidInput {
                            field: "timing".to_owned(),
                        }));
                    }
                };
                let id: AutomationId = self.mint()?;
                (
                    vec![exact(
                        "INSERT INTO automations(automation_id,target,timing_rule,anchor_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at) VALUES (?,?,?,?,NULL,?,?,'active',?,?)",
                        vec![
                            text(&id),
                            encode(target)?,
                            encode(timing)?,
                            at(),
                            text(body.as_str()),
                            text(timestamp(first)),
                            at(),
                            at(),
                        ],
                    )],
                    Answer::AutomationSet {
                        automation_id: id,
                        next_due_at: Some(first),
                        state: mail::AutomationState::Active,
                    },
                )
            }
            Op::DisableAutomation { automation_id } => {
                let rows=self.host.read(Statement{sql:"SELECT automation_id,target,timing_rule,anchor_at,evaluated_through_at,text,next_due_at,state FROM automations WHERE automation_id=?",args:vec![text(automation_id)]}).await?;
                let rows = decode_automations(&rows)?;
                let Some(row) = rows.first() else {
                    return Ok(Err(Refusal::AutomationNotFound {
                        automation_id: automation_id.clone(),
                    }));
                };
                if row.state == mail::AutomationState::Done {
                    return Ok(Err(Refusal::AutomationAlreadyDone {
                        automation_id: automation_id.clone(),
                    }));
                }
                let statements = if row.state == mail::AutomationState::Disabled {
                    vec![]
                } else {
                    vec![exact(
                        "UPDATE automations SET state='disabled',updated_at=? WHERE automation_id=? AND state='active'",
                        vec![at(), text(automation_id)],
                    )]
                };
                (
                    statements,
                    Answer::AutomationDisabled {
                        automation_id: automation_id.clone(),
                    },
                )
            }
            Op::SendMessage { .. } => return Err(corrupt("timer operation")),
        };
        Ok(Ok(decision))
    }
}
