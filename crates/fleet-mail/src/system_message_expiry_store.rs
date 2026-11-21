//! Persistence and guarded settlement for system-message expiry.
use crate::{
    message_deadline::{EXPIRED_24_HOURS, system_message_deadline},
    system_message_expiry::{State, SystemMessageExpiry},
};
use central_engine_model::{EngineTime, InstanceKey, StateTag};
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};
use fleet_engine::{
    EngineHost, LoadFailure, StoreAdapter, Versioned, decode_load_failure, guarded_attempt_sql,
    load_store_failure, raw_version,
};
use fleet_runtime_ports::CentralStore as _;
use fleet_runtime_ports::stored_values::format_utc_millis;
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    stored_values::{column, string},
    workspace_decode_error::corrupt,
};
use std::convert::Infallible;

const EXPIRY_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM mail_messages WHERE message_id IS ?20 AND message_kind IS ?21 AND state IS ?22 AND failure_reason IS ?23 AND delivery_window_ends_at IS ?24 AND created_at IS ?25 AND updated_at IS ?26)"
);

fn decode_queued(rows: &ReadOutcome, row: &[SqlValue]) -> Result<State, StoreFailure> {
    string(rows, row, "message_id")?
        .parse::<workspace_ids::MessageId>()
        .map_err(|_| corrupt("mail_messages.message_id"))?;
    if !matches!(column(rows, row, "failure_reason")?, SqlValue::Null) {
        return Err(corrupt("mail_messages.failure_reason"));
    }
    let time = |field| {
        string(rows, row, field)?
            .parse::<EngineTime>()
            .map_err(|_| corrupt(field))
    };
    let deadline = time("delivery_window_ends_at")?;
    if system_message_deadline(time("created_at")?) != Some(deadline) {
        return Err(corrupt("mail_messages.delivery_window_ends_at"));
    }
    Ok(State::Queued { deadline })
}

pub struct SystemMessageExpiryStore<'a, R: EngineHost>(&'a R);

impl<'a, R: EngineHost> SystemMessageExpiryStore<'a, R> {
    pub fn new(runtime: &'a R) -> Self {
        Self(runtime)
    }
}

impl<R: EngineHost> StoreAdapter<SystemMessageExpiry> for SystemMessageExpiryStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<State>>, LoadFailure> {
        let rows = self.0.host().read(Statement {
            sql: "SELECT message_id,message_kind,state,failure_reason,delivery_window_ends_at,created_at,updated_at FROM mail_messages WHERE message_id=? AND state='queued' AND message_kind IN ('notice','deliveryOutcome')",
            args: vec![text(&key.0)],
        }).await.map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let state = decode_queued(&rows, &version.0)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?;
        Ok(Some(Versioned { state, version }))
    }

    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<State>,
        next: &State,
    ) -> Result<GuardedStatement, StoreFailure> {
        let mut args = vec![
            text(next.state_tag()),
            text(EXPIRED_24_HOURS),
            text(format_utc_millis(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE mail_messages SET state=?1,failure_reason=?2,updated_at=?3 WHERE message_id IS ?4 AND message_kind IS ?5 AND state IS ?6 AND failure_reason IS ?7 AND delivery_window_ends_at IS ?8 AND created_at IS ?9 AND updated_at IS ?10",
            args,
        ))
    }

    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &Infallible,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match *effect {}
    }

    fn failure_sql(&self) -> &'static str {
        EXPIRY_FAILURE
    }
}
