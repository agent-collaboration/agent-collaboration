use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::{json, string};
use central_storage::{
    GuardedStatement, ReadOutcome, RowExpectation, SqlValue, Statement, StoreFailure,
};
use workspace_model::{ActorIdentityId, RequestId, WorkspaceResult};

#[derive(Clone, Copy)]
pub(crate) enum RequestLogStore {
    Organization,
    Workspace,
}

struct RequestLogQueries {
    lookup: &'static str,
    upsert: &'static str,
    prune: &'static str,
}

macro_rules! request_log_queries {
    ($table:literal) => {
        RequestLogQueries {
            lookup: concat!(
                "SELECT caller,content_digest,answer FROM ",
                $table,
                " WHERE request_id=? AND created_at>?"
            ),
            upsert: concat!(
                "INSERT INTO ",
                $table,
                " (request_id,caller,content_digest,answer,created_at,updated_at) VALUES (?,?,?,?,?,?) ",
                "ON CONFLICT(request_id) DO UPDATE SET caller=excluded.caller,content_digest=excluded.content_digest,",
                "answer=excluded.answer,created_at=excluded.created_at,updated_at=excluded.updated_at"
            ),
            prune: concat!("DELETE FROM ", $table, " WHERE created_at<=?"),
        }
    };
}

impl RequestLogStore {
    const fn queries(self) -> RequestLogQueries {
        match self {
            Self::Organization => request_log_queries!("runtime_request_log"),
            Self::Workspace => request_log_queries!("workspace_request_log"),
        }
    }
}

/// The request log's content digest; the one definition lives in workspace-model
/// so the Router's outbox keeps the same digest for answered writes.
pub(crate) fn digest(operation: &impl serde::Serialize) -> Result<String, StoreFailure> {
    workspace_model::operation_content_digest(operation).map_err(|_| corrupt("operation"))
}
pub(crate) fn lookup(store: RequestLogStore, request: &RequestId, cutoff: &str) -> Statement {
    Statement {
        sql: store.queries().lookup,
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
    let answer: WorkspaceResult =
        json(string(rows, row, "answer")?, "workspace_request_log.answer")?;
    if matches!(
        answer,
        WorkspaceResult::Conflict
            | WorkspaceResult::OutcomeUnknown
            | WorkspaceResult::CentralUnavailable
    ) {
        return Err(corrupt("workspace_request_log.answer"));
    }
    Ok(Some(answer))
}
pub(crate) fn retain(
    store: RequestLogStore,
    request: &RequestId,
    caller: &ActorIdentityId,
    digest: &str,
    answer: &impl serde::Serialize,
    now: &str,
    cutoff: &str,
) -> Result<Vec<GuardedStatement>, StoreFailure> {
    let encoded = serde_json::to_string(answer).map_err(|_| corrupt("answer"))?;
    let text = |value: &str| SqlValue::Text(value.to_owned());
    let queries = store.queries();
    Ok(vec![
        GuardedStatement {
            statement: Statement {
                sql: queries.upsert,
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
                sql: queries.prune,
                args: vec![text(cutoff)],
            },
            expect: RowExpectation::Any,
        },
    ])
}
