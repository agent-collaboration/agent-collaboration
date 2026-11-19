use central_engine_model::{EngineTime, TransitionError};
use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use fleet_contracts::central_mail_rows::timestamp;
use fleet_runtime_ports::guarded_statements::exact;
use fleet_runtime_ports::guarded_statements::{any, text};
use fleet_runtime_ports::stored_values::json;
use fleet_runtime_ports::stored_values::string;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use workspace_model::{ActorIdentityId, RequestId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum EngineRefusal<Error> {
    Domain { error: Error },
    NotAllowed { state: String, event: String },
    Stale,
}
impl<Error> From<TransitionError<Error>> for EngineRefusal<Error> {
    fn from(error: TransitionError<Error>) -> Self {
        match error {
            TransitionError::Domain(error) => Self::Domain { error },
            TransitionError::NotAllowed { state, event } => Self::NotAllowed {
                state: state.into(),
                event: event.into(),
            },
            TransitionError::Stale => Self::Stale,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase", deny_unknown_fields)]
pub enum EngineReceipt<State, Error> {
    Applied { state: State },
    Refused { error: EngineRefusal<Error> },
    NotFound,
    RequestIdReused,
}

/// One request freezes this cutoff for its lookup, expired-row delete and loser
/// reread. Millisecond formatting is shared with stored receipt timestamps.
pub(crate) struct ReceiptClaim {
    pub request: RequestId,
    pub caller: ActorIdentityId,
    pub digest: String,
    pub started_at: EngineTime,
    pub cutoff: EngineTime,
}
impl ReceiptClaim {
    pub fn lookup(&self) -> Statement {
        Statement {
            sql: "SELECT caller,content_digest,answer FROM request_log WHERE request_id=? AND created_at>=?",
            args: vec![text(&self.request), text(timestamp(self.cutoff))],
        }
    }
    pub fn replay<State: DeserializeOwned, Error: DeserializeOwned>(
        &self,
        rows: &ReadOutcome,
    ) -> Result<Option<EngineReceipt<State, Error>>, StoreFailure> {
        let row = match rows.rows.as_slice() {
            [] => return Ok(None),
            [row] => row,
            _ => return Err(corrupt("request_log.request_id")),
        };
        if string(rows, row, "caller")? != self.caller.as_str()
            || string(rows, row, "content_digest")? != self.digest
        {
            return Ok(Some(EngineReceipt::RequestIdReused));
        }
        json(string(rows, row, "answer")?, "request_log.answer").map(Some)
    }
    pub fn statements(
        &self,
        answer: &impl Serialize,
    ) -> Result<Vec<GuardedStatement>, StoreFailure> {
        let encoded = serde_json::to_string(answer).map_err(|_| corrupt("request_log.answer"))?;
        Ok(vec![
            any(
                "DELETE FROM request_log WHERE request_id=? AND created_at<?",
                vec![text(&self.request), text(timestamp(self.cutoff))],
            ),
            exact(
                "INSERT INTO request_log(request_id,caller,content_digest,answer,created_at,updated_at) VALUES (?,?,?,?,?,?)",
                vec![
                    text(&self.request),
                    text(&self.caller),
                    text(&self.digest),
                    text(encoded),
                    text(timestamp(self.started_at)),
                    text(timestamp(self.started_at)),
                ],
            ),
        ])
    }
}
#[cfg(test)]
#[path = "receipts_test.rs"]
mod tests;
