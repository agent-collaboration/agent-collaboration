use super::{automation_timer as automation, wake_timer as wake};
use crate::{
    central_mail_rows::{insert_message, timestamp},
    central_timer_rows::{decode_automations, decode_wakes},
    engine::{
        attempt_statements::guarded_attempt_sql,
        port::EngineHost,
        store_adapter::{
            LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure,
            raw_version,
        },
    },
    guarded_statements::{exact, text},
};
use central_engine_model::{InstanceKey, StateTag};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use fleet_runtime::workspace_decode_error::corrupt;
use fleet_runtime_ports::CentralStore as _;
use workspace_model::central_mail as mail;

pub(crate) struct WakeStore<'a, R: EngineHost>(pub &'a R);
pub(crate) struct AutomationStore<'a, R: EngineHost>(pub &'a R);

pub(crate) const WAKE_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM wakes WHERE wake_id IS ?20 AND target IS ?21 AND due_at IS ?22 AND text IS ?23 AND state IS ?24 AND created_at IS ?25 AND updated_at IS ?26 AND state NOT IN ('done','disabled'))"
);
pub(crate) const AUTOMATION_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM automations WHERE automation_id IS ?20 AND target IS ?21 AND timing_rule IS ?22 AND anchor_at IS ?23 AND expires_at IS ?24 AND evaluated_through_at IS ?25 AND text IS ?26 AND next_due_at IS ?27 AND state IS ?28 AND created_at IS ?29 AND updated_at IS ?30 AND state NOT IN ('done','disabled'))"
);

impl<R: EngineHost> StoreAdapter<wake::WakeTimer> for WakeStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<wake::State>>, LoadFailure> {
        let rows = self.0.host().read(Statement {
            sql: "SELECT wake_id,target,due_at,text,state,created_at,updated_at FROM wakes WHERE wake_id=?", args: vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let row = decode_wakes(&rows)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?
            .into_iter()
            .next()
            .ok_or_else(|| load_store_failure(corrupt("wakes.wake_id")))?;
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
        let mut args = vec![text(next.state_tag()), text(timestamp(self.0.now()?))];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE wakes SET state=?1,updated_at=?2 WHERE wake_id IS ?3 AND target IS ?4 AND due_at IS ?5 AND text IS ?6 AND state IS ?7 AND created_at IS ?8 AND updated_at IS ?9",
            args,
        ))
    }
    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &wake::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match effect {
            wake::Effect::QueueMessage(message) => Ok(vec![insert_message(message)?]),
        }
    }
    fn failure_sql(&self) -> &'static str {
        WAKE_FAILURE
    }
}

impl<R: EngineHost> StoreAdapter<automation::AutomationTimer> for AutomationStore<'_, R> {
    async fn load(
        &self,
        key: &InstanceKey,
    ) -> Result<Option<Versioned<automation::State>>, LoadFailure> {
        let rows = self.0.host().read(Statement {
            sql: "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at FROM automations WHERE automation_id=?", args: vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let row = decode_automations(&rows)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?
            .into_iter()
            .next()
            .ok_or_else(|| load_store_failure(corrupt("automations.automation_id")))?;
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
            evaluated.map_or(SqlValue::Null, |time| text(timestamp(time))),
            text(next.state_tag()),
            text(timestamp(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE automations SET evaluated_through_at=?1,state=?2,updated_at=?3 WHERE automation_id IS ?4 AND target IS ?5 AND timing_rule IS ?6 AND anchor_at IS ?7 AND expires_at IS ?8 AND evaluated_through_at IS ?9 AND text IS ?10 AND next_due_at IS ?11 AND state IS ?12 AND created_at IS ?13 AND updated_at IS ?14",
            args,
        ))
    }
    fn effect_statements(
        &self,
        key: &InstanceKey,
        effect: &automation::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        Ok(match effect {
            automation::Effect::QueueMessage(message) => vec![insert_message(message)?],
            automation::Effect::ScheduleAt(due) => vec![exact(
                "UPDATE automations SET next_due_at=? WHERE automation_id=?",
                vec![text(timestamp(*due)), text(&key.0)],
            )],
            automation::Effect::ClearSchedule => vec![exact(
                "UPDATE automations SET next_due_at=NULL WHERE automation_id=?",
                vec![text(&key.0)],
            )],
        })
    }
    fn failure_sql(&self) -> &'static str {
        AUTOMATION_FAILURE
    }
}
