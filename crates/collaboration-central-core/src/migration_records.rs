//! Each storage object's record of applied migrations: sqlx's standard table
//! shape, one table per migration set, so versions from different sets never
//! collide (PD1). Central's own runner keeps writing them, because Durable
//! Object storage refuses the transaction sqlx's `Migrator::run` opens
//! (sqlx-durable-object connection.rs:95–96).
use crate::generated_migration_records as queries;
pub(crate) use crate::generated_migration_records::StoredMigrationRecord;
use durable_steps::{StoreError, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// Every migration set an object can run; the name is the set's stable key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MigrationSet {
    FleetSyncLease,
    FleetSyncReceiver,
    FleetSyncSession,
    FleetSyncDelivery,
    CentralFleet,
    CentralRegistry,
    CentralSession,
}

impl MigrationSet {
    pub(crate) fn named(name: &str) -> Result<Self, StoreError> {
        Ok(match name {
            "fleet-sync-lease" => Self::FleetSyncLease,
            "fleet-sync-receiver" => Self::FleetSyncReceiver,
            "fleet-sync-session" => Self::FleetSyncSession,
            "fleet-sync-delivery" => Self::FleetSyncDelivery,
            "central-fleet" => Self::CentralFleet,
            "central-registry" => Self::CentralRegistry,
            "central-session" => Self::CentralSession,
            _ => {
                return Err(StoreError::Corrupt {
                    field: "schema.migration".into(),
                    message: format!("unknown migration set {name}"),
                });
            }
        })
    }

    #[cfg(test)]
    pub(crate) const ALL: [Self; 7] = [
        Self::FleetSyncLease,
        Self::FleetSyncReceiver,
        Self::FleetSyncSession,
        Self::FleetSyncDelivery,
        Self::CentralFleet,
        Self::CentralRegistry,
        Self::CentralSession,
    ];

    #[cfg(test)]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::FleetSyncLease => "fleet-sync-lease",
            Self::FleetSyncReceiver => "fleet-sync-receiver",
            Self::FleetSyncSession => "fleet-sync-session",
            Self::FleetSyncDelivery => "fleet-sync-delivery",
            Self::CentralFleet => "central-fleet",
            Self::CentralRegistry => "central-registry",
            Self::CentralSession => "central-session",
        }
    }

    pub(crate) const fn table(self) -> &'static str {
        match self {
            Self::FleetSyncLease => "_sqlx_migrations_fleet_sync_lease",
            Self::FleetSyncReceiver => "_sqlx_migrations_fleet_sync_receiver",
            Self::FleetSyncSession => "_sqlx_migrations_fleet_sync_session",
            Self::FleetSyncDelivery => "_sqlx_migrations_fleet_sync_delivery",
            Self::CentralFleet => "_sqlx_migrations_central_fleet",
            Self::CentralRegistry => "_sqlx_migrations_central_registry",
            Self::CentralSession => "_sqlx_migrations_central_session",
        }
    }
}

/// One record to write after its migration ran.
pub(crate) struct AppliedMigration {
    pub(crate) version: i64,
    pub(crate) description: String,
    pub(crate) installed_on: SyncTime,
    pub(crate) checksum: Vec<u8>,
    pub(crate) execution_time: i64,
}

/// sqlx's `installed_on` text, as SQLite's CURRENT_TIMESTAMP writes it.
pub(crate) fn installed_on_text(at: SyncTime) -> Result<String, StoreError> {
    chrono::DateTime::from_timestamp_millis(at.unix_millis())
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .ok_or_else(|| StoreError::backend("migration time is outside UTC range"))
}

pub(crate) async fn initialize<Scope>(
    scope: &mut Scope,
    set: MigrationSet,
) -> Result<(), StoreError>
where
    Scope: TransactionScope,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
{
    match set {
        MigrationSet::FleetSyncLease => queries::initialize_fleet_sync_lease_records(scope).await,
        MigrationSet::FleetSyncReceiver => {
            queries::initialize_fleet_sync_receiver_records(scope).await
        }
        MigrationSet::FleetSyncSession => {
            queries::initialize_fleet_sync_session_records(scope).await
        }
        MigrationSet::FleetSyncDelivery => {
            queries::initialize_fleet_sync_delivery_records(scope).await
        }
        MigrationSet::CentralFleet => queries::initialize_central_fleet_records(scope).await,
        MigrationSet::CentralRegistry => queries::initialize_central_registry_records(scope).await,
        MigrationSet::CentralSession => queries::initialize_central_session_records(scope).await,
    }
    .map_err(StoreError::backend)
}

pub(crate) async fn read<Scope>(
    scope: &mut Scope,
    set: MigrationSet,
) -> Result<Vec<StoredMigrationRecord>, sqlx_core::Error>
where
    Scope: TransactionScope,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<Vec<u8>>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<i64>: Type<Scope::Database> + Decode<'value, Scope::Database>,
{
    match set {
        MigrationSet::FleetSyncLease => queries::read_fleet_sync_lease_records(scope).await,
        MigrationSet::FleetSyncReceiver => queries::read_fleet_sync_receiver_records(scope).await,
        MigrationSet::FleetSyncSession => queries::read_fleet_sync_session_records(scope).await,
        MigrationSet::FleetSyncDelivery => queries::read_fleet_sync_delivery_records(scope).await,
        MigrationSet::CentralFleet => queries::read_central_fleet_records(scope).await,
        MigrationSet::CentralRegistry => queries::read_central_registry_records(scope).await,
        MigrationSet::CentralSession => queries::read_central_session_records(scope).await,
    }
}

pub(crate) async fn insert<Scope>(
    scope: &mut Scope,
    set: MigrationSet,
    applied: AppliedMigration,
) -> Result<(), StoreError>
where
    Scope: TransactionScope,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> i64: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Vec<u8>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let installed_on = installed_on_text(applied.installed_on)?;
    let AppliedMigration {
        version,
        description,
        checksum,
        execution_time,
        ..
    } = applied;
    match set {
        MigrationSet::FleetSyncLease => {
            queries::insert_fleet_sync_lease_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::FleetSyncReceiver => {
            queries::insert_fleet_sync_receiver_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::FleetSyncSession => {
            queries::insert_fleet_sync_session_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::FleetSyncDelivery => {
            queries::insert_fleet_sync_delivery_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::CentralFleet => {
            queries::insert_central_fleet_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::CentralRegistry => {
            queries::insert_central_registry_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
        MigrationSet::CentralSession => {
            queries::insert_central_session_record(
                scope,
                version,
                description,
                installed_on,
                checksum,
                execution_time,
            )
            .await
        }
    }
    .map_err(StoreError::backend)
}

/// A stored record in sqlx's exact shape, or the field that isn't.
pub(crate) fn validate(
    record: &StoredMigrationRecord,
    set: MigrationSet,
) -> Result<(), StoreError> {
    let installed = record.installed_on.as_deref().is_some_and(|text| {
        record.installed_class.as_deref() == Some("text")
            && chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").is_ok()
    });
    for (field, valid) in [
        (
            "version",
            record.version_class.as_deref() == Some("integer") && record.version.is_some(),
        ),
        (
            "description",
            record
                .description
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
        ),
        ("installed_on", installed),
        (
            "success",
            record.success_class.as_deref() == Some("integer") && record.success == Some(1),
        ),
        (
            "checksum",
            record.checksum_class.as_deref() == Some("blob") && record.checksum.is_some(),
        ),
        (
            "execution_time",
            record.execution_class.as_deref() == Some("integer")
                && record.execution_time.is_some_and(|elapsed| elapsed >= 0),
        ),
    ] {
        if !valid {
            return Err(StoreError::Corrupt {
                field: format!("{}.{field}", set.table()),
                message: "invalid migration record".into(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "migration_records_test.rs"]
mod tests;
