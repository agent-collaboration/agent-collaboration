use super::{
    attempt_statements::guarded_attempt_sql,
    store_adapter::{
        LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure, raw_version,
    },
};
use super::{
    port::EngineHost,
    probe_machine::{EngineProbe, ProbeEffect, ProbeState},
};
use fleet_runtime_ports::CentralStore as _;
use crate::{
    central_mail_rows::timestamp,
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, string},
};
use central_engine_model::{EngineTime, InstanceKey, StateTag};
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};

pub(crate) struct ProbeStore<'a, R: EngineHost>(pub &'a R);
const FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM engine_probe_items WHERE item_id IS ?20 AND group_id IS ?21 AND due_at IS ?22 AND claim IS ?23 AND state IS ?24 AND completed_claim IS ?25 AND created_at IS ?26 AND updated_at IS ?27 AND machine_kind='engine_probe' AND state NOT IN ('fired','stopped'))"
);

fn decode(rows: &ReadOutcome) -> Result<ProbeState, StoreFailure> {
    let row = rows
        .rows
        .first()
        .ok_or_else(|| corrupt("engine_probe_items.item_id"))?;
    let uuid = |name: &str| -> Result<uuid::Uuid, StoreFailure> {
        string(rows, row, name)?
            .parse::<uuid::Uuid>()
            .ok()
            .filter(|id| id.get_version_num() == 7)
            .ok_or_else(|| corrupt(name))
    };
    uuid("item_id")?;
    let group = uuid("group_id")?;
    let due = match column(rows, row, "due_at")? {
        SqlValue::Null => None,
        SqlValue::Text(value) => Some(
            value
                .parse::<EngineTime>()
                .ok()
                .filter(|time| central_engine_model::supported_time(*time))
                .ok_or_else(|| corrupt("engine_probe_items.due_at"))?,
        ),
        SqlValue::Integer(_) | SqlValue::Blob(_) => {
            return Err(corrupt("engine_probe_items.due_at"));
        }
    };
    let claim = string(rows, row, "claim")?.to_owned();
    if claim.is_empty() {
        return Err(corrupt("engine_probe_items.claim"));
    }
    for name in ["created_at", "updated_at"] {
        string(rows, row, name)?
            .parse::<EngineTime>()
            .ok()
            .filter(|time| central_engine_model::supported_time(*time))
            .ok_or_else(|| corrupt(name))?;
    }
    let completed = column(rows, row, "completed_claim")?;
    match string(rows, row, "state")? {
        "waiting" if *completed == SqlValue::Null => Ok(ProbeState::Waiting { group, due, claim }),
        "fired" if *completed == text(&claim) => Ok(ProbeState::Fired),
        "stopped" if *completed == SqlValue::Null => Ok(ProbeState::Stopped),
        "waiting" | "fired" | "stopped" => Err(corrupt("engine_probe_items.completed_claim")),
        _ => Err(corrupt("engine_probe_items.state")),
    }
}
impl<R: EngineHost> StoreAdapter<EngineProbe> for ProbeStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<ProbeState>>, LoadFailure> {
        let rows=self.0.host().read(Statement {
            sql:"SELECT item_id,group_id,due_at,claim,state,completed_claim,created_at,updated_at FROM engine_probe_items WHERE item_id=? AND machine_kind='engine_probe'",
            args:vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let state =
            decode(&rows).map_err(|failure| decode_load_failure(failure, version.clone()))?;
        Ok(Some(Versioned { state, version }))
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<ProbeState>,
        next: &ProbeState,
    ) -> Result<GuardedStatement, StoreFailure> {
        let mut args = vec![text(next.state_tag()), text(timestamp(self.0.now()?))];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE engine_probe_items SET state=?1,updated_at=?2 WHERE item_id IS ?3 AND group_id IS ?4 AND due_at IS ?5 AND claim IS ?6 AND state IS ?7 AND completed_claim IS ?8 AND created_at IS ?9 AND updated_at IS ?10 AND machine_kind='engine_probe'",
            args,
        ))
    }
    fn effect_statements(
        &self,
        key: &InstanceKey,
        effect: &ProbeEffect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        Ok(match effect {
            ProbeEffect::BumpGroup { group, at } => vec![exact(
                "UPDATE engine_probe_groups SET effect_count=effect_count+1,updated_at=? WHERE group_id=?",
                vec![text(timestamp(*at)), text(group)],
            )],
            ProbeEffect::Claim => vec![exact(
                "UPDATE engine_probe_items SET completed_claim=claim WHERE item_id=? AND machine_kind='engine_probe'",
                vec![text(&key.0)],
            )],
        })
    }
    fn failure_sql(&self) -> &'static str {
        FAILURE
    }
}
