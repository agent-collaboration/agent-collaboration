use crate::workspace_repository::{WorkspaceRepo, corrupt};
use fleet_contracts::{ReadWorkspaceRequestStatus, RetainedWorkspaceStatus, WorkspaceServiceRpc};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Type};
use std::marker::PhantomData;
use workspace_model::WorkspaceResult;

pub struct WorkspaceService<'connection, DB, E> {
    executor: E,
    connection: PhantomData<fn(&'connection DB)>,
}

impl<'connection, DB, E> WorkspaceService<'connection, DB, E>
where
    DB: Database,
    E: Executor<'connection, Database = DB> + Copy + Sync,
    DB::Arguments: IntoArguments<DB>,
    String: Type<DB> + for<'query> Encode<'query, DB> + for<'row> Decode<'row, DB>,
    for<'row> &'row str: ColumnIndex<DB::Row>,
{
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            connection: PhantomData,
        }
    }
}

impl<'connection, DB, E> WorkspaceServiceRpc for WorkspaceService<'connection, DB, E>
where
    DB: Database,
    E: Executor<'connection, Database = DB> + Copy + Sync,
    DB::Arguments: IntoArguments<DB>,
    String: Type<DB> + for<'query> Encode<'query, DB> + for<'row> Decode<'row, DB>,
    for<'row> &'row str: ColumnIndex<DB::Row>,
{
    async fn read_request_status(
        &self,
        call: ReadWorkspaceRequestStatus,
    ) -> Result<RetainedWorkspaceStatus, central_storage::StoreFailure> {
        let Some((caller, answer)) = WorkspaceRepo::retained_status(self.executor, &call).await?
        else {
            return Ok(RetainedWorkspaceStatus::NotFound);
        };
        // Validate the retained row even for a different actor so damaged
        // receipts consistently report a storage failure.
        let status = match answer {
            WorkspaceResult::Refused(reason) => RetainedWorkspaceStatus::Refused(reason),
            result @ (WorkspaceResult::Done(_) | WorkspaceResult::NotFound { .. }) => {
                RetainedWorkspaceStatus::Answered(Box::new(result))
            }
            _ => return Err(corrupt("request log answer")),
        };
        if caller != call.actor {
            return Ok(RetainedWorkspaceStatus::NotFound);
        }
        Ok(status)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "workspace_service_test.rs"]
mod tests;
