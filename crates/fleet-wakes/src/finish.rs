use crate::stored_rows::WAKE_FAILURE;
use central_engine_model::{EngineTime, ItemFailureClass, MachineKind};
use central_storage::ReadOutcome;
use fleet_engine::{DiscardStore, LoadFailure, Versioned, raw_version};
use fleet_runtime_ports::stored_values::format_utc_millis;
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    stored_values::string,
};
use fleet_timer_policy::{FinishKind, Lifecycle, LifecycleFinish};

pub struct WakeKind;
impl FinishKind for WakeKind {
    const KIND: MachineKind = MachineKind("wake_timer");
}
pub type WakeFinish = LifecycleFinish<WakeKind>;

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

pub fn wake_finish_store() -> DiscardStore<WakeFinish> {
    DiscardStore {
        read_sql: "SELECT wake_id,target,due_at,text,state,created_at,updated_at FROM automation_wakes WHERE wake_id=?",
        decode: |rows| decode_lifecycle(rows, "automation_wakes.state"),
        save: |prior, next, now: EngineTime| {
            let mut args = vec![
                text(central_engine_model::StateTag::state_tag(next)),
                text(format_utc_millis(now)),
            ];
            args.extend(prior.version.0.clone());
            Ok(exact(
                "UPDATE automation_wakes SET state=?1,updated_at=?2 WHERE wake_id IS ?3 AND target IS ?4 AND due_at IS ?5 AND text IS ?6 AND state IS ?7 AND created_at IS ?8 AND updated_at IS ?9",
                args,
            ))
        },
        effect_statements: |effect| match *effect {},
        failure_sql: WAKE_FAILURE,
    }
}

#[cfg(test)]
#[path = "finish_test.rs"]
mod tests;
