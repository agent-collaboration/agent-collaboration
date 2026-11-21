use crate::rows::{AUTOMATION_FAILURE, timestamp};
use central_engine_model::{EngineTime, ItemFailureClass, MachineKind};
use central_storage::ReadOutcome;
use fleet_engine::{DiscardStore, LoadFailure, Versioned, raw_version};
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    stored_values::string,
};
use fleet_timer_policy::{FinishKind, Lifecycle, LifecycleFinish};

pub struct AutomationKind;
impl FinishKind for AutomationKind {
    const KIND: MachineKind = MachineKind("automation_timer");
}
pub type AutomationFinish = LifecycleFinish<AutomationKind>;

/// Only the lifecycle column is decoded; an unknown value is the one case the
/// finishing view cannot act on, and it is never coerced.
fn lifecycle(rows: &ReadOutcome, column: &'static str) -> Result<Lifecycle, ItemFailureClass> {
    let corrupt = || ItemFailureClass::CorruptRow {
        column: column.to_owned(),
        raw_value: None,
    };
    let row = rows.rows.first().ok_or_else(corrupt)?;
    Lifecycle::from_stored_tag(string(rows, row, "state").map_err(|_| corrupt())?)
        .ok_or_else(corrupt)
}

fn decode_lifecycle(
    rows: &ReadOutcome,
    state_column: &'static str,
) -> Result<Option<Versioned<Lifecycle>>, LoadFailure> {
    let Some(version) = raw_version(rows)? else {
        return Ok(None);
    };
    match lifecycle(rows, state_column) {
        Ok(state) => Ok(Some(Versioned { state, version })),
        Err(class) => Err(LoadFailure::Corrupt { class, version }),
    }
}

pub fn automation_finish_store() -> DiscardStore<AutomationFinish> {
    DiscardStore {
        read_sql: "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at FROM automation_schedules WHERE automation_id=?",
        decode: |rows| decode_lifecycle(rows, "automation_schedules.state"),
        save: |prior, next, now: EngineTime| {
            let mut args = vec![
                text(central_engine_model::StateTag::state_tag(next)),
                text(timestamp(now)),
            ];
            args.extend(prior.version.0.clone());
            Ok(exact(
                "UPDATE automation_schedules SET state=?1,updated_at=?2 WHERE automation_id IS ?3 AND target IS ?4 AND timing_rule IS ?5 AND anchor_at IS ?6 AND expires_at IS ?7 AND evaluated_through_at IS ?8 AND text IS ?9 AND next_due_at IS ?10 AND state IS ?11 AND created_at IS ?12 AND updated_at IS ?13",
                args,
            ))
        },
        effect_statements: |effect| match *effect {},
        failure_sql: AUTOMATION_FAILURE,
    }
}
