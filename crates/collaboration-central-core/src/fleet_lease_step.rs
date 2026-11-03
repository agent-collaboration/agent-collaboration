//! Claim settlement receipts observe the lease hook's committed directory projection.
use crate::{
    ClaimReservationOutcome, ClaimReservationState,
    fleet_lease_projection_step::checked_row_id,
    fleet_record_codec::{corrupt, decode_json, required},
    generated_fleet_lease as queries,
    lease_projection::{ClaimGrantedUpdate, LeaseProjection, LeaseProjectionResult},
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
            let attempt = attempt.as_uuid().to_string();
            // The CallId identifies the original settlement across projection refreshes.
            if let Some(receipt) = queries::read_claim_settlement_receipt(
                scope,
                self.call.operation.as_uuid().to_string(),
                self.call.step.as_str().into(),
            )
            .await
            .map_err(StoreError::backend)?
            {
                checked_row_id(receipt.receipt_id, "claim.receipt_id")?;
                if required(receipt.attempt_id, "claim.attempt_id")? != attempt.as_str()
                    || required(receipt.project_id, "claim.project_id")? != project.as_str()
                {
                    return Err(corrupt(
                        "claim.receipt",
                        "CallId belongs to another settlement",
                    ));
                }
                let result: LeaseProjectionResult =
                    decode_json(&required(receipt.result, "claim.result")?, "claim.result")?;
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
            let committed: LeaseProjection = decode_json(
                &required(row.lease_projection, "claim.lease_projection")?,
                "claim.lease_projection",
            )?;
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
            if let Some(previous) = queries::read_claim_reservation(scope, attempt.as_str().into())
                .await
                .map_err(StoreError::backend)?
            {
                if required(previous.project_id, "claim.project_id")? != project.as_str() {
                    return Err(corrupt(
                        "claim.project_id",
                        "attempt belongs to another project",
                    ));
                }
                let state: ClaimReservationState =
                    decode_json(&required(previous.state, "claim.state")?, "claim.state")?;
                if state
                    == (ClaimReservationState::Settled {
                        outcome: ClaimReservationOutcome::Refused,
                    })
                {
                    return Err(corrupt("claim.state", "cannot grant a refused settlement"));
                }
            }
            queries::store_claim_reservation(
                scope,
                attempt.as_str().into(),
                project.as_str().into(),
                serde_json::to_string(&ClaimReservationState::Settled {
                    outcome: ClaimReservationOutcome::Granted,
                })
                .map_err(StoreError::backend)?,
            )
            .await
            .map_err(StoreError::backend)?;
            queries::insert_claim_settlement_receipt(
                scope,
                self.row_ids.next()?.to_string(),
                self.call.operation.as_uuid().to_string(),
                self.call.step.as_str().into(),
                attempt.as_str().into(),
                project.as_str().into(),
                serde_json::to_string(&result).map_err(StoreError::backend)?,
            )
            .await
            .map_err(StoreError::backend)?;
            let project = message_board::ProjectId::try_from(project)
                .map_err(|error| corrupt("claim.project_id", error))?;
            crate::fleet_archive_completion::complete_project(
                scope,
                &project,
                self.now,
                &mut self.row_ids,
            )
            .await?;
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver, _>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(self.now.timestamp_millis()),
            )
            .await?;
            Ok((result, Vec::new()))
        })
    }
}
