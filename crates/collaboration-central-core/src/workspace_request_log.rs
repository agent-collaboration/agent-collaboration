use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::{json, string};
use central_storage::{
    GuardedStatement, ReadOutcome, RowExpectation, SqlValue, Statement, StoreFailure,
};
use workspace_model::{ActorIdentityId, RequestId, WorkspaceResult};

pub(crate) fn digest(operation: &impl serde::Serialize) -> Result<String, StoreFailure> {
    use sha2::{Digest, Sha256};
    let encoded = serde_json_canonicalizer::to_vec(operation).map_err(|_| corrupt("operation"))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
pub(crate) fn lookup(request: &RequestId, cutoff: &str) -> Statement {
    Statement {
        sql: "SELECT caller,content_digest,answer FROM request_log WHERE request_id=? AND created_at>?",
        args: vec![
            SqlValue::Text(request.to_string()),
            SqlValue::Text(cutoff.to_owned()),
        ],
    }
}
pub(crate) fn replay(
    rows: &ReadOutcome,
    caller: &ActorIdentityId,
    digest: &str,
) -> Result<Option<WorkspaceResult>, StoreFailure> {
    let Some(row) = rows.rows.first() else {
        return Ok(None);
    };
    if string(rows, row, "caller")? != caller.as_str()
        || string(rows, row, "content_digest")? != digest
    {
        return Ok(Some(WorkspaceResult::Refused(
            workspace_model::RefusalReason::RequestIdReused,
        )));
    }
    let answer: WorkspaceResult = json(string(rows, row, "answer")?, "request_log.answer")?;
    if matches!(
        answer,
        WorkspaceResult::Conflict
            | WorkspaceResult::OutcomeUnknown
            | WorkspaceResult::CentralUnavailable
    ) {
        return Err(corrupt("request_log.answer"));
    }
    Ok(Some(answer))
}
pub(crate) fn retain(
    request: &RequestId,
    caller: &ActorIdentityId,
    digest: &str,
    answer: &impl serde::Serialize,
    now: &str,
    cutoff: &str,
) -> Result<Vec<GuardedStatement>, StoreFailure> {
    let encoded = serde_json::to_string(answer).map_err(|_| corrupt("answer"))?;
    let text = |value: &str| SqlValue::Text(value.to_owned());
    Ok(vec![
        GuardedStatement {
            statement: Statement {
                sql: "INSERT INTO request_log (request_id,caller,content_digest,answer,created_at,updated_at) VALUES (?,?,?,?,?,?) ON CONFLICT(request_id) DO UPDATE SET caller=excluded.caller,content_digest=excluded.content_digest,answer=excluded.answer,created_at=excluded.created_at,updated_at=excluded.updated_at",
                args: vec![
                    text(request.as_str()),
                    text(caller.as_str()),
                    text(digest),
                    text(&encoded),
                    text(now),
                    text(now),
                ],
            },
            expect: RowExpectation::Exactly(1),
        },
        GuardedStatement {
            statement: Statement {
                sql: "DELETE FROM request_log WHERE created_at<=?",
                args: vec![text(cutoff)],
            },
            expect: RowExpectation::Any,
        },
    ])
}
