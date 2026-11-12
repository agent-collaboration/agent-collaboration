//! Apply library-owned corpora once, in central's one owned store transaction.

use crate::{
    ObjectHost, generated_bootstrap,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, executor::Executor, types::Type,
};
use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

#[cfg(test)]
#[path = "migration_bookkeeping_test.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineSchema {
    Registry,
    Session,
    Delivery,
    Fleet,
}
impl EngineSchema {
    fn corpus(self) -> &'static str {
        match self {
            Self::Registry => "fleet-sync-lease",
            Self::Session => "fleet-sync-session",
            Self::Delivery => "fleet-sync-delivery",
            Self::Fleet => "central-fleet",
        }
    }
    fn migrations(self) -> Vec<&'static sqlx_core::migrate::Migration> {
        match self {
            Self::Registry => fleet_sync_lease::LEASE_MIGRATOR.sql_migrations().collect(),
            Self::Session => fleet_sync_session::SESSION_MIGRATOR
                .sql_migrations()
                .collect(),
            Self::Delivery => fleet_sync_delivery::DELIVERY_MIGRATOR.iter().collect(),
            Self::Fleet => FLEET_MIGRATIONS.iter().collect(),
        }
    }
}

pub struct ObjectBootstrap<Host: ObjectHost> {
    clock: Arc<dyn Clock>,
    driver: StepDriver<Host>,
    now: runtime_primitives::SyncTime,
}
impl<Host: ObjectHost> ObjectBootstrap<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn new(host: Arc<Host>, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now();
        Self {
            clock,
            driver: StepDriver::new(host),
            now,
        }
    }
    pub async fn initialize(&self, schema: EngineSchema) -> Result<usize, StoreError> {
        let (applied, _effects) = self
            .driver
            .run(BootstrapStep {
                schema,
                now: self.now,
                clock: self.clock.clone(),
            })
            .await?;
        Ok(applied.count)
    }
}

struct CorpusProgress {
    count: usize,
}

struct BootstrapApplied {
    count: usize,
}

struct BootstrapStep {
    clock: Arc<dyn Clock>,
    schema: EngineSchema,
    now: runtime_primitives::SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for BootstrapStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'blob, Driver>
        + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> i64: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'scalar, Driver>
        + sqlx_core::decode::Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'scalar, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = BootstrapApplied;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            generated_bootstrap::initialize_migration_failures(scope)
                .await
                .map_err(bootstrap_sql_error)?;
            let primary = apply_corpus(
                scope,
                self.schema.corpus(),
                self.schema.migrations(),
                self.now,
                self.clock.as_ref(),
            )
            .await?;
            if self.schema == EngineSchema::Delivery {
                fleet_sync_delivery::retire_relay_host_tables(scope)
                    .await
                    .map_err(fleet_sync_delivery::delivery_rollback)?;
            }
            let mut count = primary.count;
            if self.schema == EngineSchema::Registry {
                count += apply_corpus(
                    scope,
                    "central-registry",
                    REGISTRY_MIGRATIONS.iter().collect(),
                    self.now,
                    self.clock.as_ref(),
                )
                .await?
                .count;
            }
            if self.schema == EngineSchema::Session {
                count += apply_corpus(
                    scope,
                    "central-session",
                    SESSION_MIGRATIONS.iter().collect(),
                    self.now,
                    self.clock.as_ref(),
                )
                .await?
                .count;
            }

            Ok((BootstrapApplied { count }, Vec::new()))
        })
    }
}

static SESSION_MIGRATIONS: std::sync::LazyLock<Vec<sqlx_core::migrate::Migration>> =
    std::sync::LazyLock::new(|| {
        use sqlx_core::{
            migrate::{Migration, MigrationType},
            sql_str::SqlSafeStr,
        };
        vec![
            Migration::new(
                11,
                "machine notices".into(),
                MigrationType::Simple,
                include_str!("../session_migrations/0011_machine_notices.sql").into_sql_str(),
                false,
            ),
            Migration::new(
                12,
                "forward correlations".into(),
                MigrationType::Simple,
                include_str!("../session_migrations/0012_forward_correlations.sql").into_sql_str(),
                false,
            ),
        ]
    });

static REGISTRY_MIGRATIONS: std::sync::LazyLock<Vec<sqlx_core::migrate::Migration>> =
    std::sync::LazyLock::new(|| {
        use sqlx_core::{
            migrate::{Migration, MigrationType},
            sql_str::SqlSafeStr,
        };
        [
            (
                1,
                "machine enrollments",
                include_str!("../registry_migrations/0001_machine_enrollments.sql"),
            ),
            (
                2,
                "lease incarnation",
                include_str!("../registry_migrations/0002_lease_incarnation.sql"),
            ),
        ]
        .into_iter()
        .map(|(version, description, sql)| {
            Migration::new(
                version,
                description.into(),
                MigrationType::Simple,
                sql.into_sql_str(),
                false,
            )
        })
        .collect()
    });

static FLEET_MIGRATIONS: std::sync::LazyLock<Vec<sqlx_core::migrate::Migration>> =
    std::sync::LazyLock::new(|| {
        use sqlx_core::{
            migrate::{Migration, MigrationType},
            sql_str::SqlSafeStr,
        };
        [
            (
                1,
                "fleet decisions",
                include_str!("../core_migrations/0001_fleet_decisions.sql"),
            ),
            (
                2,
                "fleet decision receipts",
                include_str!("../core_migrations/0002_fleet_decision_receipts.sql"),
            ),
            (
                3,
                "canonical fleet records",
                include_str!("../core_migrations/0003_fleet_records.sql"),
            ),
            (
                4,
                "directory copy summary",
                include_str!("../core_migrations/0004_directory_copy_summary.sql"),
            ),
            (
                5,
                "claim lease projection",
                include_str!("../core_migrations/0005_claim_lease_projection.sql"),
            ),
            (
                6,
                "archive reports",
                include_str!("../core_migrations/0006_archive_reports.sql"),
            ),
            (
                7,
                "immutable creation bindings",
                include_str!("../core_migrations/0007_creation_decisions.sql"),
            ),
            (
                8,
                "Fleet feed readers",
                include_str!("../core_migrations/0008_feed_readers.sql"),
            ),
            // This common metadata DDL is also ensured before every corpus and
            // by the post-rollback writer. Keep one checked source for its schema.
            (
                9,
                "migration failure diagnostics",
                include_str!("../core_migrations/0009_migration_failures.sql"),
            ),
            (
                10,
                "admin imports",
                include_str!("../admin_import_migrations/0010_admin_imports.sql"),
            ),
        ]
        .into_iter()
        .map(|(version, description, sql)| {
            Migration::new(
                version,
                description.into(),
                MigrationType::Simple,
                sql.into_sql_str(),
                false,
            )
        })
        .collect()
    });

async fn apply_corpus<Scope>(
    scope: &mut Scope,
    corpus: &'static str,
    migrations: Vec<&'static sqlx_core::migrate::Migration>,
    now: runtime_primitives::SyncTime,
    clock: &dyn Clock,
) -> Result<CorpusProgress, StoreError>
where
    Scope: TransactionScope,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'blob, Scope::Database>
        + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'scalar, Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Scope::Database> + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> Option<i64>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let set = crate::migration_records::MigrationSet::named(corpus)?;
    crate::migration_records::initialize(scope, set).await?;
    let mut applied = BTreeMap::new();
    for row in crate::migration_records::read(scope, set)
        .await
        .map_err(bootstrap_sql_error)?
    {
        crate::migration_records::validate(&row, set)?;
        let version = row.version.ok_or_else(|| corrupt("missing version"))?;
        let checksum = row.checksum.ok_or_else(|| corrupt("missing checksum"))?;
        if applied.insert(version, checksum).is_some() {
            return Err(corrupt("duplicate version"));
        }
    }
    for (version, saved) in &applied {
        let migration = migrations
            .iter()
            .find(|migration| {
                migration.version == *version && migration.migration_type.is_up_migration()
            })
            .ok_or_else(|| corrupt("applied migration is absent from the corpus"))?;
        if &checksum(migration) != saved {
            return Err(corrupt("migration checksum changed"));
        }
    }
    let highest = applied.last_key_value().map(|(version, _)| *version);
    let mut newly_applied = Vec::new();
    let mut previous = None;
    for migration in migrations
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
    {
        if migration.no_tx || previous.is_some_and(|version| migration.version <= version) {
            return Err(corrupt("corpus must be transactional and ordered"));
        }
        previous = Some(migration.version);
        if applied.contains_key(&migration.version) {
            continue;
        }
        if highest.is_some_and(|version| migration.version <= version) {
            return Err(corrupt("migration inserted below an applied version"));
        }
        // Validate legacy domain rows after their old corpus prefix, before typed DDL consumes them.
        let validated = match (corpus, migration.version) {
            ("fleet-sync-lease", 5 | 6) => fleet_sync_lease::validate_legacy_rows(scope)
                .await
                .map_err(|error| error.to_string()),
            ("fleet-sync-session", 202611030001) => fleet_sync_session::validate_legacy_rows(scope)
                .await
                .map_err(|error| error.to_string()),
            _ => Ok(()),
        };
        validated.map_err(|message| {
            crate::migration_failure::MigrationFailure::rollback(
                corpus,
                migration.version,
                now,
                StoreError::Corrupt {
                    field: format!("{corpus}.legacy_rows"),
                    message,
                },
            )
        })?;
        let started = clock.now().unix_millis();
        scope
            .executor()
            .execute(migration.sql.as_ref())
            .await
            .map_err(|error| {
                crate::migration_failure::MigrationFailure::rollback(
                    corpus,
                    migration.version,
                    now,
                    bootstrap_sql_error(error),
                )
            })?;
        let installed_on = clock.now().unix_millis();
        let execution_time = installed_on.saturating_sub(started).max(0);
        crate::migration_records::insert(
            scope,
            set,
            crate::migration_records::AppliedMigration {
                version: migration.version,
                description: migration.description.to_string(),
                installed_on: runtime_primitives::SyncTime::from_unix_millis(installed_on),
                checksum: checksum(migration),
                execution_time,
            },
        )
        .await
        .map_err(|error| {
            crate::migration_failure::MigrationFailure::rollback(
                corpus,
                migration.version,
                now,
                error,
            )
        })?;
        newly_applied.push(migration.version);
    }
    let version = previous.ok_or_else(|| corrupt("corpus has no up migration"))?;
    generated_bootstrap::clear_migration_failure(scope, corpus.into())
        .await
        .map_err(|error| {
            crate::migration_failure::MigrationFailure::rollback(
                corpus,
                version,
                now,
                bootstrap_sql_error(error),
            )
        })?;
    Ok(CorpusProgress {
        count: newly_applied.len(),
    })
}

fn checksum(migration: &sqlx_core::migrate::Migration) -> Vec<u8> {
    migration.checksum.to_vec()
}

fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt {
        field: "schema.migration".into(),
        message: message.into(),
    }
}

fn bootstrap_sql_error(error: sqlx_core::Error) -> StoreError {
    fleet_sync_receiver::receiver_rollback(fleet_sync_receiver::ReceiverError::Storage(
        fleet_sync_receiver::classify_sqlx_error(error),
    ))
}

#[cfg(test)]
pub(crate) struct FixedBootstrapClock(pub runtime_primitives::SyncTime);
#[cfg(test)]
impl Clock for FixedBootstrapClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        self.0
    }
}
