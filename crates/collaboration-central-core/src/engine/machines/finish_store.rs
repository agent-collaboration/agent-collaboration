//! Finishing views over the wake and automation tables (P8 Discard). Each reads
//! the row's raw stored values as its compare-and-set version, decodes only the
//! lifecycle `state`, and saves `disabled` guarded on every raw column with IS.
use super::lifecycle_finish::{AutomationKind, Lifecycle, LifecycleFinish, NoEffect, WakeKind};
use crate::{
    central_mail_rows::timestamp,
    engine::{
        port::EngineHost,
        store_adapter::{LoadFailure, StoreAdapter, Versioned, load_store_failure, raw_version},
    },
    guarded_statements::{exact, text},
    workspace_row_codec::string,
};
use central_engine_model::{InstanceKey, ItemFailureClass};
use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use fleet_runtime_ports::CentralStore as _;

pub(crate) struct WakeFinishStore<'a, R: EngineHost>(pub &'a R);
pub(crate) struct AutomationFinishStore<'a, R: EngineHost>(pub &'a R);

/// Only the lifecycle column is decoded; an unknown value is the one case the
/// finishing view cannot act on, and it is never coerced.
fn lifecycle(rows: &ReadOutcome, column: &'static str) -> Result<Lifecycle, ItemFailureClass> {
    let corrupt = || ItemFailureClass::CorruptRow {
        column: column.to_owned(),
        raw_value: None,
    };
    let row = rows.rows.first().ok_or_else(corrupt)?;
    match string(rows, row, "state").map_err(|_| corrupt())? {
        "active" => Ok(Lifecycle::Active),
        "done" => Ok(Lifecycle::Done),
        "disabled" => Ok(Lifecycle::Disabled),
        _ => Err(corrupt()),
    }
}

async fn load_lifecycle<R: EngineHost>(
    host: &R,
    sql: &'static str,
    key: &InstanceKey,
    state_column: &'static str,
) -> Result<Option<Versioned<Lifecycle>>, LoadFailure> {
    let rows = host
        .host()
        .read(Statement {
            sql,
            args: vec![text(&key.0)],
        })
        .await
        .map_err(load_store_failure)?;
    let Some(version) = raw_version(&rows)? else {
        return Ok(None);
    };
    match lifecycle(&rows, state_column) {
        Ok(state) => Ok(Some(Versioned { state, version })),
        Err(class) => Err(LoadFailure::Corrupt { class, version }),
    }
}

impl<R: EngineHost> StoreAdapter<LifecycleFinish<WakeKind>> for WakeFinishStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<Lifecycle>>, LoadFailure> {
        load_lifecycle(
            self.0,
            "SELECT wake_id,target,due_at,text,state,created_at,updated_at FROM wakes WHERE wake_id=?",
            key,
            "wakes.state",
        )
        .await
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<Lifecycle>,
        next: &Lifecycle,
    ) -> Result<GuardedStatement, StoreFailure> {
        let mut args = vec![
            text(central_engine_model::StateTag::state_tag(next)),
            text(timestamp(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE wakes SET state=?1,updated_at=?2 WHERE wake_id IS ?3 AND target IS ?4 AND due_at IS ?5 AND text IS ?6 AND state IS ?7 AND created_at IS ?8 AND updated_at IS ?9",
            args,
        ))
    }
    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &NoEffect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match *effect {}
    }
    fn failure_sql(&self) -> &'static str {
        super::timer_store::WAKE_FAILURE
    }
}

impl<R: EngineHost> StoreAdapter<LifecycleFinish<AutomationKind>> for AutomationFinishStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<Lifecycle>>, LoadFailure> {
        load_lifecycle(
            self.0,
            "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at FROM automations WHERE automation_id=?",
            key,
            "automations.state",
        )
        .await
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<Lifecycle>,
        next: &Lifecycle,
    ) -> Result<GuardedStatement, StoreFailure> {
        let mut args = vec![
            text(central_engine_model::StateTag::state_tag(next)),
            text(timestamp(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE automations SET state=?1,updated_at=?2 WHERE automation_id IS ?3 AND target IS ?4 AND timing_rule IS ?5 AND anchor_at IS ?6 AND expires_at IS ?7 AND evaluated_through_at IS ?8 AND text IS ?9 AND next_due_at IS ?10 AND state IS ?11 AND created_at IS ?12 AND updated_at IS ?13",
            args,
        ))
    }
    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &NoEffect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match *effect {}
    }
    fn failure_sql(&self) -> &'static str {
        super::timer_store::AUTOMATION_FAILURE
    }
}
