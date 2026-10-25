//! A first native archive report and its finalizing records share one Fleet step.
use crate::{
    fleet_archive_completion,
    fleet_row_ids::FleetRowIds,
    generated_fleet_archive as queries,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_admission::{ArchiveReport, ArchiveReportOutcome, decide_archive_report};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct ReportArchive {
    pub report: ArchiveReport,
    pub row_ids: FleetRowIds,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReportArchive
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ArchiveReportOutcome;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
            let snapshot = fleet_archive_completion::snapshot(
                scope,
                &self.report.board,
                Some(&self.report.project),
            )
            .await?;
            let decision = match decide_archive_report(&self.report, &snapshot) {
                Err(refusal) => return Ok((ArchiveReportOutcome::Refused(refusal), Vec::new())),
                Ok(decision) if !decision.record => {
                    return Ok((ArchiveReportOutcome::AlreadyRecorded, Vec::new()));
                }
                Ok(decision) => decision,
            };
            queries::insert_archive_report(
                scope,
                self.row_ids.next()?.to_string(),
                self.report.board.as_str().into(),
                self.report.project.as_str().into(),
                serde_json::to_string(&self.report).map_err(StoreError::backend)?,
            )
            .await
            .map_err(StoreError::backend)?;
            if let Some(finalization) = decision.finalize {
                fleet_archive_completion::apply(
                    scope,
                    &snapshot,
                    &finalization,
                    self.report.reported_at,
                    &mut self.row_ids,
                )
                .await?;
            }
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(
                    self.report.reported_at.timestamp_millis(),
                ),
            )
            .await?;
            Ok((ArchiveReportOutcome::Recorded, Vec::new()))
        })
    }
}
