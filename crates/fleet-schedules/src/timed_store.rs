use crate::{
    automation_timer as automation,
    stored_rows::{AUTOMATION_FAILURE, decode_automations},
};
use central_engine_model::{InstanceKey, StateTag};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use fleet_engine::{
    EngineHost, LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure,
    raw_version,
};
use fleet_runtime_ports::stored_values::format_utc_millis;
use fleet_runtime_ports::{
    CentralStore as _,
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use workspace_model::central_mail as mail;

pub struct AutomationStore<'a, R: EngineHost, PlanQueued> {
    runtime: &'a R,
    plan_queued: PlanQueued,
}
impl<'a, R, PlanQueued> AutomationStore<'a, R, PlanQueued>
where
    R: EngineHost,
    PlanQueued: Fn(&mail::MessageRecord) -> Result<GuardedStatement, StoreFailure> + Send + Sync,
{
    pub fn new(runtime: &'a R, plan_queued: PlanQueued) -> Self {
        Self {
            runtime,
            plan_queued,
        }
    }
}

impl<R, PlanQueued> StoreAdapter<automation::AutomationTimer> for AutomationStore<'_, R, PlanQueued>
where
    R: EngineHost,
    PlanQueued: Fn(&mail::MessageRecord) -> Result<GuardedStatement, StoreFailure> + Send + Sync,
{
    async fn load(
        &self,
        key: &InstanceKey,
    ) -> Result<Option<Versioned<automation::State>>, LoadFailure> {
        let rows = self.runtime.host().read(Statement {
            sql: "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at FROM automation_schedules WHERE automation_id=?", args: vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let row = decode_automations(&rows)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?
            .into_iter()
            .next()
            .ok_or_else(|| load_store_failure(corrupt("automation_schedules.automation_id")))?;
        let state = match row.state {
            mail::AutomationState::Active => automation::State::Active(row),
            mail::AutomationState::Done => automation::State::Done {
                evaluated: row.evaluated,
            },
            mail::AutomationState::Disabled => automation::State::Disabled {
                evaluated: row.evaluated,
            },
        };
        Ok(Some(Versioned { state, version }))
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<automation::State>,
        next: &automation::State,
    ) -> Result<GuardedStatement, StoreFailure> {
        let evaluated = match next {
            automation::State::Active(row) => row.evaluated,
            automation::State::Done { evaluated } | automation::State::Disabled { evaluated } => {
                *evaluated
            }
        };
        let mut args = vec![
            evaluated.map_or(SqlValue::Null, |time| text(format_utc_millis(time))),
            text(next.state_tag()),
            text(format_utc_millis(self.runtime.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE automation_schedules SET evaluated_through_at=?1,state=?2,updated_at=?3 WHERE automation_id IS ?4 AND target IS ?5 AND timing_rule IS ?6 AND anchor_at IS ?7 AND expires_at IS ?8 AND evaluated_through_at IS ?9 AND text IS ?10 AND next_due_at IS ?11 AND state IS ?12 AND created_at IS ?13 AND updated_at IS ?14",
            args,
        ))
    }
    fn effect_statements(
        &self,
        key: &InstanceKey,
        effect: &automation::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        Ok(match effect {
            automation::Effect::QueueMessage(message) => vec![(self.plan_queued)(message)?],
            automation::Effect::ScheduleAt(due) => vec![exact(
                "UPDATE automation_schedules SET next_due_at=? WHERE automation_id=?",
                vec![text(format_utc_millis(*due)), text(&key.0)],
            )],
            automation::Effect::ClearSchedule => vec![exact(
                "UPDATE automation_schedules SET next_due_at=NULL WHERE automation_id=?",
                vec![text(&key.0)],
            )],
        })
    }
    fn failure_sql(&self) -> &'static str {
        AUTOMATION_FAILURE
    }
}
