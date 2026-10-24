//! Projection order and the separately receipted claim settle in one local transaction.
use crate::{
    ClaimReservationOutcome, ClaimReservationState, RegistryIncarnation,
    fleet_lease_projection_step::{apply_projection, checked_row_id, read_incarnation_fence},
    fleet_record_codec::{corrupt, decode_json, required},
    generated_fleet_lease as queries,
    lease_projection::{ClaimGrantedUpdate, LeaseProjectionResult},
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallId, StoreError, Uuid, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct AdvanceLeaseIncarnationStep {
    pub at_least: u64,
    pub row_id: Uuid,
}
pub(crate) struct StoreClaimGrant {
    pub update: ClaimGrantedUpdate,
    pub call: CallId,
    pub now: chrono::DateTime<chrono::Utc>,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver>
    for AdvanceLeaseIncarnationStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RegistryIncarnation;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let previous = read_incarnation_fence(scope).await?;
            let next = previous
                .as_ref()
                .map_or(0, |(_, value)| value.value())
                .max(self.at_least)
                .checked_add(1)
                .ok_or_else(|| corrupt("incarnation.value", "counter exhausted"))?;
            let incarnation = RegistryIncarnation::new(next)
                .map_err(|error| corrupt("incarnation.value", error))?;
            if let Some((row_id, _)) = previous {
                queries::store_lease_incarnation(scope, next.to_string(), row_id).await
            } else {
                queries::insert_lease_incarnation(scope, self.row_id.to_string(), next.to_string())
                    .await
            }
            .map_err(StoreError::backend)?;
            Ok((incarnation, Vec::new()))
        })
    }
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
                if matches!(
                    result,
                    LeaseProjectionResult::Fenced { .. } | LeaseProjectionResult::Conflict { .. }
                ) {
                    return Err(corrupt(
                        "claim.result",
                        "retryable publication was receipted",
                    ));
                }
                return Ok((result, Vec::new()));
            }
            let result = apply_projection(scope, &projection).await?;
            if matches!(
                result,
                LeaseProjectionResult::Fenced { .. } | LeaseProjectionResult::Conflict { .. }
            ) {
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
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver>(
                scope,
                previous_feed,
                runtime_primitives::SyncTime::from_unix_millis(self.now.timestamp_millis()),
            )
            .await?;
            Ok((result, Vec::new()))
        })
    }
}
