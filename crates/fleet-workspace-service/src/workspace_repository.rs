use central_storage::{CorruptReason, StoreFailure};
use fleet_contracts::ReadWorkspaceRequestStatus;
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Type};
use workspace_model::{ActorIdentityId, WorkspaceResult};

#[derive(sqlx::FromRow)]
struct StoredWorkspaceReceipt {
    caller: String,
    answer: String,
}

pub(crate) struct WorkspaceRepo;

impl WorkspaceRepo {
    pub(crate) async fn retained_status<'connection, DB, E>(
        executor: E,
        request: &ReadWorkspaceRequestStatus,
    ) -> Result<Option<(ActorIdentityId, WorkspaceResult)>, StoreFailure>
    where
        DB: Database,
        E: Executor<'connection, Database = DB>,
        DB::Arguments: IntoArguments<DB>,
        String: Type<DB> + for<'query> Encode<'query, DB> + for<'row> Decode<'row, DB>,
        for<'row> &'row str: ColumnIndex<DB::Row>,
    {
        let request_id = request.request_id.to_string();
        let cutoff = request.cutoff.to_rfc3339();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let _ = sqlx::query_file_as!(
            StoredWorkspaceReceipt,
            "src/queries/read_request_status.sql",
            &request_id,
            &cutoff
        );

        let rows = sqlx::query_as::<DB, StoredWorkspaceReceipt>(include_str!(
            "queries/read_request_status.sql"
        ))
        .bind(request_id)
        .bind(cutoff)
        .fetch_all(executor)
        .await
        .map_err(query_failure)?;
        let row = match rows.as_slice() {
            [] => return Ok(None),
            [row] => row,
            _ => return Err(corrupt("request log request_id")),
        };
        let caller = row
            .caller
            .parse()
            .map_err(|_| corrupt("request log caller"))?;
        let answer = serde_json::from_str(&row.answer)
            .map_err(|_| corrupt("workspace_request_log.answer"))?;
        Ok(Some((caller, answer)))
    }
}

fn query_failure(error: sqlx::Error) -> StoreFailure {
    match error {
        sqlx::Error::ColumnDecode { index, .. } => corrupt(index.trim_matches('"')),
        sqlx::Error::ColumnNotFound(column) => StoreFailure::Corrupt {
            reason: CorruptReason::MissingColumn { column },
        },
        _ => StoreFailure::Unavailable,
    }
}

pub(crate) fn corrupt(column: &str) -> StoreFailure {
    StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: column.to_owned(),
        },
    }
}
