//! The direct message reducer's row codec over `messages`. The version is the
//! message's own columns; the sender's session is read from actor_identities,
//! which owns the actor-to-session mapping, and is not part of the version.
use super::direct_message::{self as direct, DirectMessage, DirectMessageSubject};
use crate::ObjectHost as _;
use crate::{
    central_mail_rows::{insert_message, timestamp},
    direct_message_rows::{
        DIRECT_MESSAGE_COLUMNS, DirectMessageStatus, decode_direct_message, decode_sender_session,
        refusal_text,
    },
    engine::{
        attempt_statements::guarded_attempt_sql,
        port::EngineHost,
        store_adapter::{
            LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure,
            raw_version,
        },
    },
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use central_engine_model::{InstanceKey, StateTag};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};

pub(crate) struct DirectMessageStore<'a, R: EngineHost>(pub &'a R);

pub(crate) const DIRECT_MESSAGE_FAILURE: &str = guarded_attempt_sql!(
    "EXISTS(SELECT 1 FROM messages WHERE message_id IS ?20 AND sender IS ?21 AND message_kind IS ?22 AND state IS ?23 AND failure_reason IS ?24 AND cancel_due_at IS ?25 AND sender_request_id IS ?26 AND created_at IS ?27 AND updated_at IS ?28 AND state NOT IN ('delivered','failed'))"
);

impl<R: EngineHost> StoreAdapter<DirectMessage> for DirectMessageStore<'_, R> {
    async fn load(
        &self,
        key: &InstanceKey,
    ) -> Result<Option<Versioned<direct::State>>, LoadFailure> {
        let rows = self
            .0
            .host()
            .read(Statement {
                sql: DIRECT_MESSAGE_COLUMNS,
                args: vec![text(&key.0)],
            })
            .await
            .map_err(load_store_failure)?;
        let Some(version) = raw_version(&rows)? else {
            return Ok(None);
        };
        let row = decode_direct_message(&rows, &version.0)
            .map_err(|failure| decode_load_failure(failure, version.clone()))?;
        let subject = |sender_session| DirectMessageSubject {
            id: row.id.clone(),
            sender_session,
            sender_request_id: row.sender_request_id.clone(),
            cancel_due_at: row.cancel_due_at,
        };
        let state = match row.status {
            DirectMessageStatus::Delivered => direct::State::Delivered,
            DirectMessageStatus::Failed(reason) => direct::State::Failed(reason),
            DirectMessageStatus::Queued | DirectMessageStatus::Cancelled => {
                let identity = self
                    .0
                    .host()
                    .read(Statement {
                        sql: "SELECT identity_kind,identity FROM actor_identities WHERE actor_identity_id=?",
                        args: vec![text(&row.sender)],
                    })
                    .await
                    .map_err(load_store_failure)?;
                let session = decode_sender_session(&identity)
                    .map_err(|failure| decode_load_failure(failure, version.clone()))?;
                if row.status == DirectMessageStatus::Queued {
                    direct::State::Queued(subject(session))
                } else {
                    direct::State::Cancelled(subject(session))
                }
            }
        };
        Ok(Some(Versioned { state, version }))
    }
    fn save(
        &self,
        _key: &InstanceKey,
        prior: &Versioned<direct::State>,
        next: &direct::State,
    ) -> Result<GuardedStatement, StoreFailure> {
        let failure = match next {
            direct::State::Failed(reason) => text(refusal_text(*reason)?),
            direct::State::Queued(_) | direct::State::Cancelled(_) | direct::State::Delivered => {
                SqlValue::Null
            }
        };
        let mut args = vec![
            text(next.state_tag()),
            failure,
            text(timestamp(self.0.now()?)),
        ];
        args.extend(prior.version.0.clone());
        if args.len() != 12 {
            return Err(corrupt("messages.version"));
        }
        Ok(exact(
            "UPDATE messages SET state=?1,failure_reason=?2,updated_at=?3 WHERE message_id IS ?4 AND sender IS ?5 AND message_kind IS ?6 AND state IS ?7 AND failure_reason IS ?8 AND cancel_due_at IS ?9 AND sender_request_id IS ?10 AND created_at IS ?11 AND updated_at IS ?12",
            args,
        ))
    }
    fn effect_statements(
        &self,
        _key: &InstanceKey,
        effect: &direct::Effect,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        match effect {
            // An outcome message has no cancel mark and no outcome of its own.
            direct::Effect::TellSender(notice) => Ok(vec![insert_message(notice)?]),
        }
    }
    fn failure_sql(&self) -> &'static str {
        DIRECT_MESSAGE_FAILURE
    }
}
