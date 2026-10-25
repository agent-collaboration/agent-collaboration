//! Ordinary publication and claim settlement share the same fenced projection write.
use crate::{
    RegistryIncarnation,
    fleet_record_codec::{corrupt, decode_json, required},
    generated_fleet_lease as queries,
    lease_projection::{LeaseProjection, LeaseProjectionResult},
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, Uuid, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct StoreLeaseProjection {
    pub projection: LeaseProjection,
    pub now: chrono::DateTime<chrono::Utc>,
    pub row_ids: crate::fleet_row_ids::FleetRowIds,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for StoreLeaseProjection
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
            let result = apply_projection(scope, &self.projection).await?;
            if !matches!(
                result,
                LeaseProjectionResult::Fenced { .. } | LeaseProjectionResult::Conflict { .. }
            ) {
                let project = message_board::ProjectId::try_from(
                    self.projection.database().as_uuid().to_string(),
                )
                .map_err(|error| corrupt("lease.project", error))?;
                crate::fleet_archive_completion::complete_project(
                    scope,
                    &project,
                    self.now,
                    &mut self.row_ids,
                )
                .await?;
            }
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
pub(crate) async fn read_incarnation_fence<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Option<(String, RegistryIncarnation)>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_lease_incarnation(scope)
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let row_id = checked_row_id(row.fence_id, "incarnation.fence_id")?;
    let value = required(row.incarnation, "incarnation.value")?;
    let incarnation = RegistryIncarnation::new(
        value
            .parse::<u64>()
            .map_err(|error| corrupt("incarnation.value", error))?,
    )
    .map_err(|error| corrupt("incarnation.value", error))?;
    if incarnation.value().to_string() != value {
        return Err(corrupt("incarnation.value", "noncanonical integer"));
    }
    Ok(Some((row_id, incarnation)))
}
pub(crate) async fn apply_projection<Scope: TransactionScope>(
    scope: &mut Scope,
    projection: &LeaseProjection,
) -> Result<LeaseProjectionResult, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let (_, fence) = read_incarnation_fence(scope)
        .await?
        .ok_or_else(|| corrupt("incarnation", "Registry has not acquired its incarnation"))?;
    if projection.incarnation() < fence {
        return Ok(LeaseProjectionResult::Fenced { fence });
    }
    let project = projection.database().as_uuid().to_string();
    let row = queries::read_directory_lease(scope, project.clone())
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| corrupt("lease.project", "directory entry is missing"))?;
    if required(row.project_id, "directory.project_id")? != project {
        return Err(corrupt("directory.project_id", "lookup identity differs"));
    }
    let previous: Option<LeaseProjection> = row
        .lease_projection
        .map(|value| decode_json(&value, "directory.lease_projection"))
        .transpose()?;
    let result = match previous {
        Some(stored) => {
            if stored.database() != projection.database() {
                return Err(corrupt(
                    "directory.lease_projection",
                    "project identity differs",
                ));
            }
            if projection.version() < stored.version() {
                LeaseProjectionResult::Older {
                    stored: stored.version(),
                }
            } else if projection.version() == stored.version() {
                if projection.digest() == stored.digest() {
                    LeaseProjectionResult::Duplicate
                } else {
                    LeaseProjectionResult::Conflict {
                        stored: stored.version(),
                    }
                }
            } else {
                LeaseProjectionResult::Applied
            }
        }
        None => LeaseProjectionResult::Applied,
    };
    if result == LeaseProjectionResult::Applied {
        queries::store_directory_lease(
            scope,
            serde_json::to_string(projection).map_err(StoreError::backend)?,
            project,
        )
        .await
        .map_err(StoreError::backend)?;
    }
    Ok(result)
}
pub(crate) fn checked_row_id(
    value: Option<String>,
    field: &'static str,
) -> Result<String, StoreError> {
    let value = required(value, field)?;
    let id = Uuid::parse_str(&value).map_err(|error| corrupt(field, error))?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(corrupt(field, "expected canonical UUIDv7"));
    }
    Ok(value)
}
