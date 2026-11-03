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

/// Lease facts, directory metadata and both ownership notices share the lease transaction.
pub(crate) struct FleetLeaseChangeHook<Store, NoticeTask> {
    ids: std::sync::Arc<dyn runtime_primitives::IdSource>,
    marker: std::marker::PhantomData<fn() -> (Store, NoticeTask)>,
}
impl<Store, NoticeTask> FleetLeaseChangeHook<Store, NoticeTask> {
    pub(crate) fn new(ids: std::sync::Arc<dyn runtime_primitives::IdSource>) -> Self {
        Self {
            ids,
            marker: std::marker::PhantomData,
        }
    }
}
impl<Store, Driver, NoticeTask, Scope> fleet_sync_lease::LeaseChangeHook<Scope>
    for FleetLeaseChangeHook<Store, NoticeTask>
where
    Store: AtomicStore,
    Scope: TransactionScope<Database = Driver> + durable_steps::JournalScope,
    Driver: Database,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
    NoticeTask: durable_steps::Task<Input = crate::machine_notice_contract::MachineNoticeAppend>,
{
    async fn on_lease_change(
        &self,
        scope: &mut Scope,
        change: &fleet_sync_lease::LeaseChange,
    ) -> Result<(), fleet_sync_lease::LeaseError> {
        use crate::machine_notice_contract::MachineNoticeAppend;
        use durable_steps::JournalScope;
        let applied = async {
            let now = change.after.changed_at;
            let clock = chrono::DateTime::from_timestamp_millis(now.unix_millis())
                .ok_or_else(|| corrupt("lease.changed_at", "timestamp is outside UTC range"))?;
            let mut row_ids = crate::fleet_row_ids::FleetRowIds::new(self.ids.clone());
            let previous_feed = crate::fleet_feed_store::high_water(scope).await?;
            let previous = crate::generated_registry_lease::read_registry_incarnation(scope)
                .await
                .map_err(StoreError::backend)?;
            let (row_id, current) = match previous {
                None => (row_ids.next()?.to_string(), 0_u64),
                Some(row) => {
                    let row_id = checked_row_id(row.row_id, "lease_incarnation.id")?;
                    let value = required(row.incarnation, "lease_incarnation.value")?;
                    let counter = value
                        .parse::<u64>()
                        .map_err(|error| corrupt("lease_incarnation.value", error))?;
                    if counter.to_string() != value {
                        return Err(corrupt("lease_incarnation.value", "noncanonical counter"));
                    }
                    (row_id, counter)
                }
            };
            let next = current
                .checked_add(1)
                .filter(|value| i64::try_from(*value).is_ok())
                .ok_or_else(|| corrupt("lease_incarnation.value", "counter exhausted"))?;
            crate::generated_registry_lease::store_registry_incarnation(
                scope,
                row_id,
                next.to_string(),
                clock.to_rfc3339(),
            )
            .await
            .map_err(StoreError::backend)?;
            let holder = match &change.after.grant {
                None => None,
                Some(grant) => {
                    let mut rows = crate::generated_enrollment::read_enrollments(
                        scope,
                        None,
                        None,
                        None,
                        Some(grant.holder.to_string()),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    if rows.len() != 1 {
                        return Err(corrupt("lease.holder", "holder is not enrolled"));
                    }
                    Some(
                        crate::machine_enrollment_record::identity_from_stored_enrollment(
                            rows.pop().ok_or_else(|| {
                                corrupt("lease.holder", "holder mapping disappeared")
                            })?,
                        )?
                        .origin
                        .installation,
                    )
                }
            };
            let projection = LeaseProjection::from_snapshot(
                change.after.clone(),
                holder,
                RegistryIncarnation::new(next)
                    .map_err(|error| corrupt("lease_incarnation.value", error))?,
            )
            .map_err(StoreError::backend)?;
            if apply_projection(scope, &projection).await? != LeaseProjectionResult::Applied {
                return Err(corrupt(
                    "lease.projection",
                    "a new lease revision did not advance the directory",
                ));
            }
            let before = change.before.as_ref().map(|snapshot| {
                (
                    snapshot.revision.epoch,
                    snapshot.grant.as_ref().map(|grant| grant.holder),
                )
            });
            let after = (
                change.after.revision.epoch,
                change.after.grant.as_ref().map(|grant| grant.holder),
            );
            if before != Some(after) {
                let project = message_board::ProjectId::try_from(change.after.database.to_string())
                    .map_err(StoreError::backend)?;
                let mut notices = Vec::new();
                if let Some(previous) = change
                    .before
                    .as_ref()
                    .and_then(|snapshot| snapshot.grant.as_ref())
                {
                    notices.push((
                        previous.holder,
                        board_sync_protocol::BoardNotice::LeaseSuperseded {
                            project: project.clone(),
                            epoch: previous.epoch,
                        },
                    ));
                }
                if let Some(current) = &change.after.grant {
                    notices.push((
                        current.holder,
                        board_sync_protocol::BoardNotice::LeaseGranted {
                            project: project.clone(),
                            epoch: current.epoch,
                        },
                    ));
                }
                for (peer, notice) in notices {
                    let id = row_ids.next()?;
                    let (notice, _) = crate::machine_notice_store::append(
                        scope,
                        peer,
                        id.to_string(),
                        notice,
                        now,
                    )
                    .await?;
                    let key = durable_steps::RequestKey::new(format!("machine-control-hint:{id}"))
                        .map_err(StoreError::backend)?;
                    scope
                        .admit::<NoticeTask>(&key, &MachineNoticeAppend::Hint { peer, notice })
                        .await
                        .map_err(StoreError::backend)?;
                    scope.ensure_wake(now).await?;
                }
            }
            let project = message_board::ProjectId::try_from(change.after.database.to_string())
                .map_err(StoreError::backend)?;
            crate::fleet_archive_completion::complete_project(scope, &project, clock, &mut row_ids)
                .await?;
            crate::fleet_notice_admission::admit_mutation_notices::<Store, Driver, _>(
                scope,
                previous_feed,
                now,
            )
            .await?;
            Ok::<_, StoreError>(())
        }
        .await;
        applied.map_err(|error| fleet_sync_lease::LeaseError::Storage {
            message: error.to_string(),
        })
    }
}
