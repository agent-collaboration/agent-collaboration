//! Test-support store adapter that runs the owed-reply reducer through `handle`
//! over the probe tables (P10). Rows carry machine_kind='owed_reply'; the
//! production table and link wiring belong to deliverable 4.
use super::{
    attempt_statements::guarded_attempt_sql,
    store_adapter::{
        LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure, raw_version,
    },
};
use super::{
    machines::owed_reply::{Effect, Finish, OwedReply, State},
    port::EngineHost,
};
use fleet_runtime_ports::CentralStore as _;
use crate::{
    central_mail_rows::timestamp,
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::{column, string},
};
use central_engine_model::{EngineTime, InstanceKey};
use central_storage::{GuardedStatement, ReadOutcome, SqlValue, Statement, StoreFailure};

pub(crate) struct OwedReplyStore<'a, R: EngineHost>(pub &'a R);
const FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM engine_probe_items WHERE item_id IS ?20 AND group_id IS ?21 AND claim IS ?22 AND state IS ?23 AND completed_claim IS ?24 AND send_id IS ?25 AND connection_id IS ?26 AND created_at IS ?27 AND updated_at IS ?28 AND machine_kind='owed_reply' AND state IN ('waiting','in_flight'))"
);
const COLUMNS: &str = "SELECT item_id,group_id,claim,state,completed_claim,send_id,connection_id,created_at,updated_at FROM engine_probe_items WHERE item_id=? AND machine_kind='owed_reply'";

/// The stored state column. Done is split by its finish so the codec is total.
fn stored_state(state: &State) -> &'static str {
    match state {
        State::Waiting => "waiting",
        State::InFlight { .. } => "in_flight",
        State::Done {
            finish: Finish::Delivered,
        } => "delivered",
        State::Done {
            finish: Finish::Refused,
        } => "refused",
        State::Expired => "expired",
    }
}

fn decode(rows: &ReadOutcome) -> Result<State, StoreFailure> {
    let row = rows
        .rows
        .first()
        .ok_or_else(|| corrupt("engine_probe_items.item_id"))?;
    let uuid = |name: &'static str| -> Result<uuid::Uuid, StoreFailure> {
        string(rows, row, name)?
            .parse::<uuid::Uuid>()
            .ok()
            .filter(|id| id.get_version_num() == 7)
            .ok_or_else(|| corrupt(name))
    };
    uuid("item_id")?;
    uuid("group_id")?;
    for name in ["created_at", "updated_at"] {
        string(rows, row, name)?
            .parse::<EngineTime>()
            .ok()
            .filter(|time| central_engine_model::supported_time(*time))
            .ok_or_else(|| corrupt(name))?;
    }
    let claim = string(rows, row, "claim")?;
    let null = |name| column(rows, row, name).map(|value| *value == SqlValue::Null);
    let finished = *column(rows, row, "completed_claim")? == text(claim);
    let unsent = null("send_id")? && null("connection_id")?;
    match string(rows, row, "state")? {
        "waiting" if unsent && null("completed_claim")? => Ok(State::Waiting),
        "in_flight" if null("completed_claim")? => Ok(State::InFlight {
            send_id: uuid("send_id")?,
            connection: uuid("connection_id")?,
        }),
        "delivered" if finished => Ok(State::Done {
            finish: Finish::Delivered,
        }),
        "refused" if finished => Ok(State::Done {
            finish: Finish::Refused,
        }),
        "expired" if null("completed_claim")? => Ok(State::Expired),
        "waiting" | "in_flight" | "delivered" | "refused" | "expired" => {
            Err(corrupt("engine_probe_items.completed_claim"))
        }
        _ => Err(corrupt("engine_probe_items.state")),
    }
}

impl<R: EngineHost> StoreAdapter<OwedReply> for OwedReplyStore<'_, R> {
    async fn load(&self, key: &InstanceKey) -> Result<Option<Versioned<State>>, LoadFailure> {
        let rows = self
            .0
            .host()
            .read(Statement {
                sql: COLUMNS,
                args: vec![text(&key.0)],
            })
            .await
            .map_err(load_store_failure)?;
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
        prior: &Versioned<State>,
        next: &State,
    ) -> Result<GuardedStatement, StoreFailure> {
        let (send, connection) = match next {
            State::InFlight {
                send_id,
                connection,
            } => (text(send_id), text(connection)),
            State::Waiting | State::Done { .. } | State::Expired => {
                (SqlValue::Null, SqlValue::Null)
            }
        };
        let mut args = vec![
            text(stored_state(next)),
            send,
            connection,
            text(timestamp(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        Ok(exact(
            "UPDATE engine_probe_items SET state=?1,send_id=?2,connection_id=?3,updated_at=?4 WHERE item_id IS ?5 AND group_id IS ?6 AND claim IS ?7 AND state IS ?8 AND completed_claim IS ?9 AND send_id IS ?10 AND connection_id IS ?11 AND created_at IS ?12 AND updated_at IS ?13 AND machine_kind='owed_reply'",
            args,
        ))
    }
    fn effect_statements(
        &self,
        key: &InstanceKey,
        effect: &Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        let now = timestamp(self.0.now()?);
        Ok(match effect {
            // One send is counted on the item's group.
            Effect::SendItem { .. } => vec![exact(
                "UPDATE engine_probe_groups SET effect_count=effect_count+1,updated_at=? WHERE group_id=(SELECT group_id FROM engine_probe_items WHERE item_id=? AND machine_kind='owed_reply')",
                vec![text(&now), text(&key.0)],
            )],
            // The unique completed claim makes a second finish impossible.
            Effect::Finish(_) => vec![exact(
                "UPDATE engine_probe_items SET completed_claim=claim WHERE item_id=? AND machine_kind='owed_reply'",
                vec![text(&key.0)],
            )],
        })
    }
    fn failure_sql(&self) -> &'static str {
        FAILURE
    }
}
