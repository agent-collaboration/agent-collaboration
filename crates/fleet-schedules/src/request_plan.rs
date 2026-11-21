use crate::rows::decode_automations;
use crate::rows::timestamp;
use agent_automation::TimingRule;
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use mail::{CentralMailAnswer as Answer, CentralMailRefusal as Refusal};
use workspace_ids::{AutomationId, SessionRef};
use workspace_model::{PostText, Timestamp, central_mail as mail};
type TimerPlan = Result<
    Result<(Vec<GuardedStatement>, mail::CentralMailAnswer), mail::CentralMailRefusal>,
    StoreFailure,
>;

fn encode<T: serde::Serialize>(value: &T) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(text)
        .map_err(|_| corrupt("mail JSON"))
}

pub fn plan_set_automation(
    target: &SessionRef,
    timing: &TimingRule,
    body: &PostText,
    now: Timestamp,
    mint: impl FnOnce() -> Result<AutomationId, StoreFailure>,
) -> TimerPlan {
    let at = || text(timestamp(now));
    let first = match crate::central_mail_time::first_due(timing, now) {
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
    let Some(expires_at) = fleet_timer_policy::lifetime_end(now) else {
        return Ok(Err(Refusal::TimeOutOfRange {
            field: "anchorAt".to_owned(),
        }));
    };
    if first > expires_at {
        return Ok(Err(Refusal::TimingViolation {
            field: match timing {
                TimingRule::At { .. } => "timing.at",
                TimingRule::After { .. } | TimingRule::Interval { .. } => "timing.seconds",
                TimingRule::Cron { .. } => "timing.expression",
            }
            .to_owned(),
            requirement: mail::TimingRequirement::WithinSevenDays,
        }));
    }
    let Some(anchor) = central_engine_model::aligned_at_or_after(now, 300) else {
        return Ok(Err(Refusal::TimeOutOfRange {
            field: "anchorAt".to_owned(),
        }));
    };
    let id: AutomationId = mint()?;
    Ok(Ok((
        vec![exact(
            "INSERT INTO automation_schedules(automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at) VALUES (?,?,?,?,?,NULL,?,?,'active',?,?)",
            vec![
                text(&id),
                encode(target)?,
                encode(timing)?,
                text(timestamp(anchor)),
                text(timestamp(expires_at)),
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
    )))
}

pub fn disable_automation_read(automation_id: &AutomationId) -> Statement {
    Statement {
        sql: "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at FROM automation_schedules WHERE automation_id=?",
        args: vec![text(automation_id)],
    }
}

pub fn plan_disable_automation(
    automation_id: &AutomationId,
    now: Timestamp,
    rows: &ReadOutcome,
) -> TimerPlan {
    let at = || text(timestamp(now));
    let rows = decode_automations(rows)?;
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
    let mut statements = if row.state == mail::AutomationState::Disabled {
        vec![]
    } else {
        vec![exact(
            "UPDATE automation_schedules SET state='disabled',updated_at=? WHERE automation_id=? AND state='active'",
            vec![at(), text(automation_id)],
        )]
    };
    statements.push(fleet_engine::delete_attempt(
        &fleet_contracts::EngineSubject {
            kind: central_engine_model::MachineKind("automation_timer"),
            key: central_engine_model::InstanceKey(automation_id.to_string()),
        },
    ));
    Ok(Ok((
        statements,
        Answer::AutomationDisabled {
            automation_id: automation_id.clone(),
        },
    )))
}
