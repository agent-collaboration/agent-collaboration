use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use fleet_runtime_ports::stored_values::string;
use fleet_runtime_ports::stored_values::{encode_json, format_utc_millis};
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use mail::{CentralMailAnswer as Answer, CentralMailRefusal as Refusal};
use workspace_ids::{SessionRef, WakeId};
use workspace_model::{PostText, Timestamp, central_mail as mail};
/// Storage failures are outer errors and retained refusals are inner results.
/// The caller owns admission and commit.
type WakePlan = Result<
    Result<(Vec<GuardedStatement>, mail::CentralMailAnswer), mail::CentralMailRefusal>,
    StoreFailure,
>;

pub fn plan_set_wake(
    target: &SessionRef,
    due_at: Timestamp,
    body: &PostText,
    now: Timestamp,
    mint: impl FnOnce() -> Result<WakeId, StoreFailure>,
) -> WakePlan {
    let at = || text(format_utc_millis(now));
    let id: WakeId = mint()?;
    Ok(Ok((
        vec![exact(
            "INSERT INTO automation_wakes(wake_id,target,due_at,text,state,created_at,updated_at) VALUES (?,?,?,?,'active',?,?)",
            vec![
                text(&id),
                encode_json(target, "mail JSON")?,
                text(format_utc_millis(due_at)),
                text(body.as_str()),
                at(),
                at(),
            ],
        )],
        Answer::WakeSet {
            wake_id: id,
            due_at,
        },
    )))
}

pub fn cancel_wake_read(wake_id: &WakeId) -> Statement {
    Statement {
        sql: "SELECT state FROM automation_wakes WHERE wake_id=?",
        args: vec![text(wake_id)],
    }
}

pub fn plan_cancel_wake(wake_id: &WakeId, now: Timestamp, rows: &ReadOutcome) -> WakePlan {
    let at = || text(format_utc_millis(now));
    let Some(row) = rows.rows.first() else {
        return Ok(Err(Refusal::WakeNotFound {
            wake_id: wake_id.clone(),
        }));
    };
    let state: mail::WakeState = serde_json::from_value(serde_json::Value::String(
        string(rows, row, "state")?.to_owned(),
    ))
    .map_err(|_| corrupt("automation_wakes.state"))?;
    if state == mail::WakeState::Done {
        return Ok(Err(Refusal::WakeAlreadyFired {
            wake_id: wake_id.clone(),
        }));
    }
    let mut statements = if state == mail::WakeState::Disabled {
        vec![]
    } else {
        vec![exact(
            "UPDATE automation_wakes SET state='disabled',updated_at=? WHERE wake_id=? AND state='active'",
            vec![at(), text(wake_id)],
        )]
    };
    statements.push(fleet_engine::delete_attempt(
        &fleet_contracts::EngineSubject {
            kind: central_engine_model::MachineKind("wake_timer"),
            key: central_engine_model::InstanceKey(wake_id.to_string()),
        },
    ));
    Ok(Ok((
        statements,
        Answer::WakeCancelled {
            wake_id: wake_id.clone(),
        },
    )))
}
