use crate::workspace_repository::WorkspaceRepo;
use fleet_contracts::{
    ReadWorkspaceRequestStatus, RetainedWorkspaceStatus, RetainedWriteAnswer, WorkspaceServiceRpc,
    WorkspaceStatusFailure,
};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Type};
use std::marker::PhantomData;
use workspace_model::{WorkspaceAnswer, WorkspaceResult};

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
    type Failure = WorkspaceStatusFailure;

    async fn read_request_status(
        &self,
        call: ReadWorkspaceRequestStatus,
    ) -> Result<RetainedWorkspaceStatus, Self::Failure> {
        let Some((caller, answer)) = WorkspaceRepo::retained_status(self.executor, &call).await?
        else {
            return Ok(RetainedWorkspaceStatus::NotFound);
        };
        // Validate the retained row even for a different actor so damaged
        // receipts consistently report a storage failure.
        let status = match answer {
            WorkspaceResult::Refused(reason) => RetainedWorkspaceStatus::Refused(reason),
            WorkspaceResult::Done(answer) => {
                RetainedWorkspaceStatus::Done(Box::new(retained_write_answer(answer)?))
            }
            WorkspaceResult::NotFound { workspace_id, item } => {
                RetainedWorkspaceStatus::ItemNotFound { workspace_id, item }
            }
            WorkspaceResult::Conflict
            | WorkspaceResult::OutcomeUnknown
            | WorkspaceResult::CentralUnavailable => return Err(WorkspaceStatusFailure::Corrupt),
        };
        if caller != call.actor {
            return Ok(RetainedWorkspaceStatus::NotFound);
        }
        Ok(status)
    }
}

fn retained_write_answer(
    answer: WorkspaceAnswer,
) -> Result<RetainedWriteAnswer, WorkspaceStatusFailure> {
    Ok(match answer {
        WorkspaceAnswer::Created { item, position } => {
            RetainedWriteAnswer::Created { item, position }
        }
        WorkspaceAnswer::Updated { item, position } => {
            RetainedWriteAnswer::Updated { item, position }
        }
        WorkspaceAnswer::Removed { item, position } => {
            RetainedWriteAnswer::Removed { item, position }
        }
        WorkspaceAnswer::Marked { scope, through } => {
            RetainedWriteAnswer::Marked { scope, through }
        }
        WorkspaceAnswer::Unchanged { item } => RetainedWriteAnswer::Unchanged { item },
        WorkspaceAnswer::MessageStatus { .. }
        | WorkspaceAnswer::WorkspaceCreated { .. }
        | WorkspaceAnswer::WorkspaceUpdated { .. }
        | WorkspaceAnswer::Item { .. }
        | WorkspaceAnswer::Page { .. }
        | WorkspaceAnswer::References { .. }
        | WorkspaceAnswer::Links { .. }
        | WorkspaceAnswer::Inbox { .. } => return Err(WorkspaceStatusFailure::Corrupt),
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "workspace_service_test.rs"]
mod tests;
