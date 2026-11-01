//! Catalog mutations and their replay receipts commit through the Fleet's one step.
use crate::automation_schedule_store::{self as schedules, ScheduleEdit};
use crate::automation_wake_store::{self as wakes, WakeAction};
use crate::{
    automation_instruction_store as instructions, automation_operation_store as operations,
    automation_revision_store as revisions,
    automation_store_error::AutomationStoreResult,
    fleet_row_ids::FleetRowIds,
    step_driver::{StepCallback, StepResult},
};
use collaboration_protocol::{AutomationAnswer, AutomationCall, AutomationRefusal, OperationId};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct AutomationCatalogStep {
    pub call: AutomationCall,
    pub installation: ServiceId,
    pub now_ms: i64,
    pub row_ids: FleetRowIds,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AutomationCatalogStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = AutomationAnswer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            crate::automation_batch_retention::prune(scope, self.now_ms).await?;
            let mutation = mutation_id(&self.call)
                .map(|operation_id| {
                    operations::request_digest(&self.call).map(|digest| (operation_id, digest))
                })
                .transpose();
            let mutation = match mutation {
                Ok(mutation) => mutation,
                Err(error) => return Ok((error.into_answer()?, Vec::new())),
            };
            if let Some((operation_id, digest)) = &mutation {
                match operations::read(scope, operation_id).await {
                    Ok(Some((previous_digest, answer))) => {
                        let answer = if previous_digest == *digest {
                            answer
                        } else {
                            AutomationAnswer::Refused(AutomationRefusal::OperationConflict {
                                operation_id: operation_id.clone(),
                            })
                        };
                        return Ok((answer, Vec::new()));
                    }
                    Ok(None) => {}
                    Err(error) => return Ok((error.into_answer()?, Vec::new())),
                }
            }
            let result: AutomationStoreResult<AutomationAnswer> = async {
                match &self.call {
                    AutomationCall::InstructionCreate(request) => {
                        let snapshot = instructions::create(
                            scope,
                            &request.text,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await?;
                        Ok(AutomationAnswer::Instruction(snapshot))
                    }
                    AutomationCall::InstructionUpdate(request) => {
                        instructions::update(scope, request, self.now_ms, &mut self.row_ids).await
                    }
                    AutomationCall::InstructionShow(request) => Ok(
                        match instructions::read(scope, &request.instruction_id).await? {
                            Some(snapshot) => AutomationAnswer::Instruction(snapshot),
                            None => AutomationAnswer::Refused(AutomationRefusal::NotFound),
                        },
                    ),
                    AutomationCall::InstructionList(request) => {
                        instructions::list(scope, request).await
                    }
                    AutomationCall::RevisionList(request) => revisions::list(scope, request).await,
                    AutomationCall::List(request) => {
                        crate::automation_catalog_list::list(scope, request).await
                    }
                    AutomationCall::ScheduleCreate(request) => {
                        schedules::create(
                            scope,
                            request,
                            self.installation,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::ScheduleShow(request) => {
                        schedules::show(scope, &request.schedule_id).await
                    }
                    AutomationCall::ScheduleUpdate(request) => {
                        schedules::change(
                            scope,
                            &request.schedule_id,
                            ScheduleEdit::Replace(request),
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::ScheduleBindPrepared(request) => {
                        schedules::bind_prepared(
                            scope,
                            request,
                            &self.installation,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::SchedulePackageImport(request) => {
                        crate::automation_schedule_package_import::import_package(
                            scope,
                            request,
                            &self.installation,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::ScheduleEnable(request) => {
                        schedules::change(
                            scope,
                            &request.schedule_id,
                            ScheduleEdit::SetEnabled(true),
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::ScheduleDisable(request) => {
                        schedules::change(
                            scope,
                            &request.schedule_id,
                            ScheduleEdit::SetEnabled(false),
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::ScheduleCancel(request) => {
                        schedules::change(
                            scope,
                            &request.schedule_id,
                            ScheduleEdit::Cancel,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::WakeSend(request) => {
                        wakes::create(
                            scope,
                            request,
                            self.installation,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::WakeShow(request) => wakes::show(scope, request).await,
                    AutomationCall::WakePause(request) => {
                        wakes::change(
                            scope,
                            request,
                            WakeAction::Pause,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::WakeResume(request) => {
                        wakes::change(
                            scope,
                            request,
                            WakeAction::Resume,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::WakeCancel(request) => {
                        wakes::change(
                            scope,
                            request,
                            WakeAction::Cancel,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::FiringShow { firing_id } => Ok(
                        match crate::automation_firing_store::read(scope, firing_id).await? {
                            Some(snapshot) => AutomationAnswer::Firing(snapshot),
                            None => AutomationAnswer::Refused(AutomationRefusal::NotFound),
                        },
                    ),
                    AutomationCall::FiringList(request) => {
                        crate::automation_firing_store::list(scope, request).await
                    }
                    AutomationCall::PullAutomationBatches(request) => {
                        crate::automation_capability_store::write(
                            scope,
                            self.installation.clone(),
                            &request.work_protocols,
                            self.now_ms,
                        )
                        .await?;
                        crate::automation_batch_custody::pull(scope, &self.installation, request)
                            .await
                    }
                    AutomationCall::AckAutomationBatches(request) => {
                        crate::automation_batch_custody::acknowledge(
                            scope,
                            &self.installation,
                            request,
                            self.now_ms,
                        )
                        .await
                    }
                    AutomationCall::FetchContents(request) => {
                        crate::automation_content_fetch::fetch(scope, &self.installation, request)
                            .await
                    }
                    AutomationCall::ReportWorkItems(request) => {
                        crate::automation_custody_reports::report::<Store, Driver>(
                            scope,
                            &self.installation,
                            request,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::LocalRunSettled(request) => {
                        crate::automation_custody_reports::settle_external::<Store, Driver>(
                            scope,
                            &self.installation,
                            request,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::RemoteSessionCreate(request) => {
                        super::remote_session_create::admit_session_create::<Store, Driver>(
                            scope,
                            &self.installation,
                            request,
                            self.now_ms,
                            &mut self.row_ids,
                        )
                        .await
                    }
                    AutomationCall::RemoteSessionCreateStatus { operation_id } => {
                        super::remote_session_create::read_session_create_status(
                            scope,
                            operation_id,
                        )
                        .await
                    }
                }
            }
            .await;
            let answer = match result {
                Ok(answer) => answer,
                Err(error) => error.into_answer()?,
            };
            if let Some((operation_id, digest)) = &mutation {
                // A failed receipt must roll back the mutation, including serialization failures.
                operations::record(scope, operation_id, digest, &answer)
                    .await
                    .map_err(StoreError::backend)?;
            }
            crate::automation_clock_deadline::request_next(scope).await?;
            Ok((answer, Vec::new()))
        })
    }
}

fn mutation_id(call: &AutomationCall) -> Option<OperationId> {
    match call {
        AutomationCall::InstructionCreate(request) => Some(request.operation_id.clone()),
        AutomationCall::InstructionUpdate(request) => Some(request.operation_id.clone()),
        AutomationCall::ScheduleCreate(request) => Some(request.operation_id.clone()),
        AutomationCall::ScheduleUpdate(request) => Some(request.operation_id.clone()),
        AutomationCall::ScheduleBindPrepared(request) => Some(request.operation_id.clone()),
        AutomationCall::SchedulePackageImport(request) => Some(request.operation_id.clone()),
        AutomationCall::ScheduleEnable(request)
        | AutomationCall::ScheduleDisable(request)
        | AutomationCall::ScheduleCancel(request) => Some(request.operation_id.clone()),
        AutomationCall::WakeSend(request) => Some(request.operation_id.clone()),
        AutomationCall::WakePause(request)
        | AutomationCall::WakeResume(request)
        | AutomationCall::WakeCancel(request) => Some(request.operation_id.clone()),
        _ => None,
    }
}
