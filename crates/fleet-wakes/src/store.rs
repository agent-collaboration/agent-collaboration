use crate::{
    engine::machines::wake_timer as wake,
    rows::{WAKE_FAILURE, decode_wakes, timestamp},
};
use central_engine_model::{InstanceKey, StateTag};
use central_storage::{GuardedStatement, Statement, StoreFailure};
use fleet_engine::{
    EngineHost, LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure,
    raw_version,
};
use fleet_runtime_ports::{
    CentralStore as _,
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use workspace_model::central_mail as mail;

pub struct WakeStore<'a, R: EngineHost, PlanQueued> {
    runtime: &'a R,
    plan_queued: PlanQueued,
}
impl<'a, R: EngineHost, PlanQueued> WakeStore<'a, R, PlanQueued> {
    pub fn new(runtime: &'a R, plan_queued: PlanQueued) -> Self {
        Self {
            runtime,
            plan_queued,
        }
    }
}

impl<R, PlanQueued> StoreAdapter<wake::WakeTimer> for WakeStore<'_, R, PlanQueued>
where
    R: EngineHost,
    PlanQueued: Fn(&mail::MessageRecord) -> Result<GuardedStatement, StoreFailure> + Send + Sync,
{
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<wake::State>>, LoadFailure> {
        let rows = self.runtime.host().read(Statement {
            sql: "SELECT wake_id,target,due_at,text,state,created_at,updated_at FROM automation_wakes WHERE wake_id=?", args: vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let row = decode_wakes(&rows)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?
            .into_iter()
            .next()
            .ok_or_else(|| load_store_failure(corrupt("automation_wakes.wake_id")))?;
        let state = match row.state {
            mail::WakeState::Active => wake::State::Active(row),
            mail::WakeState::Done => wake::State::Done,
            mail::WakeState::Disabled => wake::State::Disabled,
        };
        Ok(Some(Versioned { state, version }))
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<wake::State>,
        next: &wake::State,
    ) -> Result<GuardedStatement, StoreFailure> {
        let mut args = vec![text(next.state_tag()), text(timestamp(self.runtime.now()?))];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE automation_wakes SET state=?1,updated_at=?2 WHERE wake_id IS ?3 AND target IS ?4 AND due_at IS ?5 AND text IS ?6 AND state IS ?7 AND created_at IS ?8 AND updated_at IS ?9",
            args,
        ))
    }
    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &wake::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match effect {
            wake::Effect::QueueMessage(message) => Ok(vec![(self.plan_queued)(message)?]),
        }
    }
    fn failure_sql(&self) -> &'static str {
        WAKE_FAILURE
    }
}
