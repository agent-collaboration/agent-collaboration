//! Lease changes project directory state and ownership notices in their own transaction.
use crate::{
    RegistryIncarnation,
    fleet_record_codec::{corrupt, required},
    generated_fleet_lease as queries,
    lease_projection::{LeaseProjection, LeaseProjectionResult},
};
use durable_steps::{AtomicStore, StoreError, Uuid, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn apply_projection<Scope: TransactionScope>(
    scope: &mut Scope,
    projection: &LeaseProjection,
    now: chrono::DateTime<chrono::Utc>,
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
    if required(row.project_id.clone(), "directory.project_id")? != project {
        return Err(corrupt("directory.project_id", "lookup identity differs"));
    }
    let previous = crate::fleet_lease_columns::decode(row)?;
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
        let columns = crate::fleet_lease_columns::LeaseColumns::encode(projection);
        queries::store_directory_lease(
            scope,
            Some(columns.epoch),
            Some(columns.change),
            Some(columns.incarnation),
            Some(columns.changed_at),
            Some(columns.lifecycle_kind),
            columns.origin_kind,
            columns.origin_epoch,
            columns.origin_sequence,
            columns.import_digest,
            columns.retiring_attempt_id,
            columns.retired_at,
            columns.holder_machine_id,
            columns.holder_service_id,
            columns.expires_at,
            Some(columns.digest),
            now.timestamp_millis().to_string(),
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
impl<Store, Driver, NoticeTask, Scope> fleet_project_lease::LeaseChangeHook<Scope>
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
        change: &fleet_project_lease::LeaseChange,
    ) -> Result<(), fleet_project_lease::LeaseError> {
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
            if apply_projection(scope, &projection, clock).await? != LeaseProjectionResult::Applied
            {
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
                // Appended here; the Fleet's notice push sends them after commit.
                for (peer, notice) in notices {
                    let id = row_ids.next()?;
                    crate::machine_notice_store::append(scope, peer, id.to_string(), notice, now)
                        .await?;
                }
            }
            let project = message_board::ProjectId::try_from(change.after.database.to_string())
                .map_err(StoreError::backend)?;
            crate::fleet_archive_completion::complete_project(scope, &project, clock, &mut row_ids)
                .await?;
            crate::fleet_notice_admission::admit_mutation_notices::<Driver, _>(
                scope,
                previous_feed,
                now,
                &mut row_ids,
            )
            .await?;
            Ok::<_, StoreError>(())
        }
        .await;
        applied.map_err(|error| fleet_project_lease::LeaseError::Storage {
            message: error.to_string(),
        })
    }
}
