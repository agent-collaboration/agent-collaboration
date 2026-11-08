//! Claim settlement receipts observe the lease hook's committed directory projection.
use crate::{
    ClaimReservationOutcome, ClaimReservationState,
    fleet_record_codec::corrupt,
    generated_fleet_lease as queries,
    lease_projection::{ClaimGrantedUpdate, LeaseProjectionResult},
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallId, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct StoreClaimGrant {
    pub update: ClaimGrantedUpdate,
    pub call: CallId,
    pub now: chrono::DateTime<chrono::Utc>,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for StoreClaimGrant
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = LeaseProjectionResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        mut self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
            let ClaimGrantedUpdate {
                attempt,
                projection,
            } = self.update;
            let project = projection.database().as_uuid().to_string();
            let attempt_id = attempt;
            // The CallId identifies the original settlement across projection refreshes.
            if let Some(receipt) = crate::fleet_claim_store::read_receipt(scope, &self.call).await?
            {
                if receipt.attempt != attempt_id || receipt.database != projection.database() {
                    return Err(corrupt(
                        "claim.receipt",
                        "CallId belongs to another settlement",
                    ));
                }
                let result = receipt.result;
                if matches!(result, LeaseProjectionResult::Conflict { .. }) {
                    return Err(corrupt(
                        "claim.result",
                        "retryable publication was receipted",
                    ));
                }
                return Ok((result, Vec::new()));
            }
            let row = queries::read_directory_lease(scope, project.clone())
                .await
                .map_err(StoreError::backend)?
                .ok_or_else(|| corrupt("claim.project", "directory entry is missing"))?;
            let committed = crate::fleet_lease_columns::decode(row)?
                .ok_or_else(|| corrupt("claim.lease_projection", "missing committed projection"))?;
            if committed.database() != projection.database() {
                return Err(corrupt(
                    "claim.project",
                    "projection belongs to another project",
                ));
            }
            // Only the lease hook writes this view. Settlement can observe an equal or newer revision.
            let result = if projection.version() < committed.version() {
                LeaseProjectionResult::Older {
                    stored: committed.version(),
                }
            } else if projection.version() == committed.version()
                && projection.digest() == committed.digest()
            {
                LeaseProjectionResult::Duplicate
            } else {
                LeaseProjectionResult::Conflict {
                    stored: committed.version(),
                }
            };
            if matches!(result, LeaseProjectionResult::Conflict { .. }) {
                return Ok((result, Vec::new()));
            }
            if matches!(
                crate::fleet_claim_store::read_reservation(
                    scope,
                    projection.database(),
                    attempt_id
                )
                .await?,
                Some(ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Refused
                })
            ) {
                return Err(corrupt("claim.state", "cannot grant a refused settlement"));
            }
            crate::fleet_claim_store::write_reservation(
                scope,
                projection.database(),
                attempt_id,
                ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Granted,
                },
                self.now,
            )
            .await?;
            crate::fleet_claim_store::insert_receipt(
                scope,
                crate::fleet_claim_store::ClaimReceiptInput {
                    id: self.row_ids.next()?,
                    call: self.call,
                    attempt: attempt_id,
                    database: projection.database(),
                    result: result.clone(),
                    now: self.now,
                },
            )
            .await?;
            let project = message_board::ProjectId::try_from(project)
                .map_err(|error| corrupt("claim.project_id", error))?;
            crate::fleet_archive_completion::complete_project(
                scope,
                &project,
                self.now,
                &mut self.row_ids,
            )
            .await?;
            crate::fleet_notice_admission::admit_mutation_notices::<Driver, _>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(self.now.timestamp_millis()),
                &mut self.row_ids,
            )
            .await?;
            Ok((result, Vec::new()))
        })
    }
}
