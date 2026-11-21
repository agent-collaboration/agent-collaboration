//! A queued notice or delivery outcome reaches its delivery deadline here.
use crate::{
    central_mail_rows::timestamp,
    message_deadline::{EXPIRED_24_HOURS, system_message_deadline},
};
use central_engine_model::{
    Context, EngineTime, EventTag, InstanceKey, Machine, MachineKind, RefusalTag, StateTag,
    Transition, TransitionError, TransitionResult,
};
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};
use fleet_engine::{
    EngineHost, LoadFailure, StoreAdapter, Versioned, decode_load_failure, guarded_attempt_sql,
    load_store_failure, raw_version,
};
use fleet_runtime_ports::CentralStore as _;
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    stored_values::{column, string},
    workspace_decode_error::corrupt,
};
use std::convert::Infallible;

pub(crate) struct SystemMessageExpiry;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Queued { deadline: EngineTime },
    Cancelled,
}

impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Queued { .. } => "queued",
            Self::Cancelled => "cancelled",
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExpireAt(pub(crate) EngineTime);

impl EventTag for ExpireAt {
    fn event_tag(&self) -> &'static str {
        "expire"
    }
}

#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ExpiryRefusal {}

impl RefusalTag for ExpiryRefusal {
    fn refusal_tag(&self) -> &'static str {
        match *self {}
    }
}

impl Machine for SystemMessageExpiry {
    const KIND: MachineKind = MachineKind("system_message_expiry");
    type State = State;
    type Event = ExpireAt;
    type Effect = Infallible;
    type Error = ExpiryRefusal;

    fn transition(
        state: &State,
        event: ExpireAt,
        context: &mut Context<'_>,
    ) -> TransitionResult<Self> {
        match state {
            State::Queued { deadline } if *deadline == event.0 && *deadline <= context.now => {
                Ok((State::Cancelled, vec![]))
            }
            State::Queued { .. } | State::Cancelled => Err(TransitionError::Stale),
        }
    }

    fn transitions() -> &'static [Transition] {
        &[Transition {
            from: "queued",
            event: "expire",
            to: "cancelled",
            terminal: true,
        }]
    }

    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Queued { deadline } => Some(*deadline),
            State::Cancelled => None,
        }
    }
}

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

pub(crate) struct SystemMessageExpiryStore<'a, R: EngineHost>(pub &'a R);

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
            text(timestamp(self.0.now()?)),
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

#[cfg(test)]
#[path = "system_message_expiry_test.rs"]
mod tests;
