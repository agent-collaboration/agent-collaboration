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

#[cfg(test)]
#[path = "fleet_workspace_columns_test.rs"]
pub(crate) mod workspace_columns_tests;

#[cfg(test)]
#[path = "fleet_board_columns_test.rs"]
mod board_columns_tests;

#[cfg(test)]
#[path = "fleet_adr_columns_test.rs"]
pub(crate) mod adr_columns_tests;

#[cfg(test)]
#[path = "fleet_repository_columns_test.rs"]
mod repository_columns_tests;

#[cfg(test)]
#[path = "fleet_directory_columns_test.rs"]
mod directory_columns_tests;

#[cfg(test)]
#[path = "fleet_feed_columns_test.rs"]
mod feed_columns_tests;

#[cfg(test)]
#[path = "fleet_feed_retention_test.rs"]
mod feed_retention_tests;

#[cfg(test)]
#[path = "fleet_ledger_columns_test.rs"]
pub(crate) mod ledger_columns_tests;

#[cfg(test)]
#[path = "fleet_ledger_integrity_test.rs"]
mod ledger_integrity_tests;

#[cfg(test)]
#[path = "fleet_receipt_columns_test.rs"]
mod receipt_columns_tests;

#[cfg(test)]
#[path = "admin_import_columns_test.rs"]
mod admin_import_columns_tests;

#[cfg(test)]
#[path = "admin_import_corruption_test.rs"]
mod admin_import_corruption_tests;

#[cfg(feature = "conformance")]
mod conformance;
#[cfg(feature = "conformance")]
pub use conformance::{
    BootstrapCommand, BootstrapHistory, BootstrapProbe, BootstrapProbeReply, BootstrapReply,
    HistoryCorruption,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineSchema {
    Registry,
    Copy,
    Session,
    Delivery,
    Fleet,
}
impl EngineSchema {
    fn corpus(self) -> &'static str {
        match self {
            Self::Registry => "fleet-sync-lease",
            Self::Copy => "fleet-sync-receiver",
            Self::Session => "fleet-sync-session",
            Self::Delivery => "fleet-sync-delivery",
            Self::Fleet => "central-fleet",
        }
    }
    fn migrations(self) -> Vec<&'static sqlx_core::migrate::Migration> {
        match self {
            Self::Registry => fleet_sync_lease::LEASE_MIGRATOR.sql_migrations().collect(),
            Self::Copy => fleet_sync_receiver::RECEIVER_MIGRATOR.iter().collect(),
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

    pub(crate) async fn migrate_copy(&self) -> crate::owner_operation_calls::CopyMigrationResult {
        use crate::owner_operation_calls::CopyMigrationResult;
        match self
            .driver
            .run(MigrateCopyStep {
                now: self.now,
                clock: self.clock.clone(),
            })
            .await
        {
            Ok((result, _effects)) => result,
            Err(error) => match crate::migration_failure::MigrationFailure::from_rollback(&error) {
                Some(failure) => {
                    // Before the first receiver migration there is no trusted
                    // version. In particular, an existing-object collision is
                    // a refusal, never shape-based adoption (decision 580).
                    let first = EngineSchema::Copy
                        .migrations()
                        .into_iter()
                        .filter(|migration| migration.migration_type.is_up_migration())
                        .map(|migration| migration.version)
                        .min();
                    let version = if failure.corpus == EngineSchema::Copy.corpus()
                        && first.and_then(|version| u64::try_from(version).ok())
                            == Some(failure.version)
                    {
                        None
                    } else {
                        Some(failure.version)
                    };
                    CopyMigrationResult::Failed {
                        corpus: failure.corpus,
                        version,
                        reason: failure.reason,
                    }
                }
                None => CopyMigrationResult::Failed {
                    corpus: EngineSchema::Copy.corpus().into(),
                    version: None,
                    reason: match fleet_sync_receiver::classify_copy_store_error(error.clone()) {
                        fleet_sync_receiver::CopyTransactionFailure::Unavailable(
                            fleet_sync_protocol::CopyUnavailableReason::Damaged { detail },
                        ) => detail,
                        _ => error.to_string(),
                    },
                },
            },
        }
    }
}

struct CorpusProgress {
    count: usize,
    from: Option<u64>,
    to: u64,
}

struct BootstrapApplied {
    count: usize,
    primary: CorpusProgress,
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
            generated_bootstrap::initialize_migration_history(scope)
                .await
                .map_err(bootstrap_sql_error)?;
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
            if self.schema == EngineSchema::Copy {
                let rules = crate::CopyReceiverRules::new(
                    runtime_primitives::SyncTime::from_unix_millis(0),
                    None,
                )?;
                fleet_sync_receiver::initialize_hook(scope, &rules)
                    .await
                    .map_err(|error| {
                        crate::migration_failure::MigrationFailure::rollback(
                            crate::copy_receiver_rules::COPY_SUMMARY_CORPUS,
                            crate::copy_receiver_rules::COPY_SUMMARY_VERSION,
                            self.now,
                            fleet_sync_receiver::receiver_rollback(error),
                        )
                    })?;
                generated_bootstrap::clear_migration_failure(
                    scope,
                    crate::copy_receiver_rules::COPY_SUMMARY_CORPUS.into(),
                )
                .await
                .map_err(|error| {
                    crate::migration_failure::MigrationFailure::rollback(
                        crate::copy_receiver_rules::COPY_SUMMARY_CORPUS,
                        crate::copy_receiver_rules::COPY_SUMMARY_VERSION,
                        self.now,
                        bootstrap_sql_error(error),
                    )
                })?;
            }
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

            Ok((BootstrapApplied { count, primary }, Vec::new()))
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

/// Today's registry corpus plus B3's staged v3 and v4 (b3-sql rev .4). Not
/// registered: only fixtures initialize it, until the B3 reader slice moves
/// the files into `registry_migrations/` and registers them.
#[cfg(any(test, feature = "conformance"))]
static STAGED_B3_REGISTRY_MIGRATIONS: std::sync::LazyLock<Vec<sqlx_core::migrate::Migration>> =
    std::sync::LazyLock::new(|| {
        use sqlx_core::{
            migrate::{Migration, MigrationType},
            sql_str::SqlSafeStr,
        };
        let staged = [
            (
                3,
                "machine installations",
                include_str!("../staged_registry_migrations/0003_machine_installations.sql"),
            ),
            (
                4,
                "machine enrollments and allowances",
                include_str!("../staged_registry_migrations/0004_machine_enrollments.sql"),
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
        });
        REGISTRY_MIGRATIONS.iter().cloned().chain(staged).collect()
    });

/// A fixture organization store as it stands before B3: the central-fleet
/// corpus, then today's central-registry corpus (v1, v2), in the live order.
#[cfg(any(test, feature = "conformance"))]
pub async fn initialize_pre_b3_fleet<Scope>(
    scope: &mut Scope,
    now: runtime_primitives::SyncTime,
    clock: &dyn Clock,
) -> Result<(), StoreError>
where
    Scope: TransactionScope + durable_steps::JournalScope,
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
    generated_bootstrap::initialize_migration_history(scope)
        .await
        .map_err(bootstrap_sql_error)?;
    generated_bootstrap::initialize_migration_failures(scope)
        .await
        .map_err(bootstrap_sql_error)?;
    apply_corpus(
        scope,
        "central-fleet",
        FLEET_MIGRATIONS.iter().collect(),
        now,
        clock,
    )
    .await?;
    apply_corpus(
        scope,
        "central-registry",
        REGISTRY_MIGRATIONS.iter().collect(),
        now,
        clock,
    )
    .await?;
    Ok(())
}

/// Initialize the registry corpus with B3's staged migrations, in the caller's
/// transaction: a fixture's stand-in for B3's deploy. Returns how many
/// migrations it applied.
#[cfg(any(test, feature = "conformance"))]
pub async fn initialize_staged_b3_registry<Scope>(
    scope: &mut Scope,
    now: runtime_primitives::SyncTime,
    clock: &dyn Clock,
) -> Result<usize, StoreError>
where
    Scope: TransactionScope + durable_steps::JournalScope,
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
    generated_bootstrap::initialize_migration_history(scope)
        .await
        .map_err(bootstrap_sql_error)?;
    generated_bootstrap::initialize_migration_failures(scope)
        .await
        .map_err(bootstrap_sql_error)?;
    Ok(apply_corpus(
        scope,
        "central-registry",
        STAGED_B3_REGISTRY_MIGRATIONS.iter().collect(),
        now,
        clock,
    )
    .await?
    .count)
}
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
            (
                11,
                "organization identity",
                include_str!("../organization_migrations/0011_organization_identity.sql"),
            ),
            (
                12,
                "organization change records",
                include_str!("../organization_migrations/0012_organization_change_records.sql"),
            ),
            (
                13,
                "organization authorization floors",
                include_str!(
                    "../organization_migrations/0013_organization_authorization_floors.sql"
                ),
            ),
            (
                14,
                "person organization authorization floors",
                include_str!(
                    "../organization_migrations/0014_person_organization_authorization_floors.sql"
                ),
            ),
            (
                15,
                "organization used requests",
                include_str!("../organization_migrations/0015_organization_used_requests.sql"),
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
    Scope: TransactionScope + durable_steps::JournalScope,
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
    let mut applied = BTreeMap::new();
    for row in generated_bootstrap::read_migration_history(scope, corpus.into())
        .await
        .map_err(bootstrap_sql_error)?
    {
        crate::migration_bookkeeping::validate_history_times(&row)?;
        crate::fleet_record_codec::check_private_id(
            &row.row_id
                .ok_or_else(|| corrupt("missing physical row identity"))?,
            "migration row identity",
        )?;
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
        // B3's switch converts today's machine rows around its v3 SQL.
        let conversion = match (corpus, migration.version) {
            ("central-registry", 3) => Some(
                crate::b3_registry_conversion::prepare(scope)
                    .await
                    .map_err(|error| {
                        crate::migration_failure::MigrationFailure::rollback(
                            corpus,
                            migration.version,
                            now,
                            error,
                        )
                    })?,
            ),
            _ => None,
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
        if let Some(conversion) = conversion {
            crate::b3_registry_conversion::apply(scope, conversion, now.unix_millis())
                .await
                .map_err(|error| {
                    crate::migration_failure::MigrationFailure::rollback(
                        corpus,
                        migration.version,
                        now,
                        error,
                    )
                })?;
        }
        let installed_on = clock.now().unix_millis();
        let execution_time = installed_on.saturating_sub(started).max(0);
        generated_bootstrap::insert_migration_history(
            scope,
            durable_steps::Uuid::now_v7().to_string(),
            corpus.into(),
            migration.version,
            migration.description.to_string(),
            installed_on,
            checksum(migration),
            execution_time,
            now.unix_millis(),
        )
        .await
        .map_err(|error| {
            crate::migration_failure::MigrationFailure::rollback(
                corpus,
                migration.version,
                now,
                bootstrap_sql_error(error),
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
        from: highest
            .map(u64::try_from)
            .transpose()
            .map_err(StoreError::backend)?,
        to: u64::try_from(version).map_err(StoreError::backend)?,
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

struct MigrateCopyStep {
    clock: Arc<dyn Clock>,
    now: runtime_primitives::SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for MigrateCopyStep
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
    type Output = crate::owner_operation_calls::CopyMigrationResult;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            use crate::owner_operation_calls::CopyMigrationResult;
            // Retired is observed inside the same transaction, before even
            // migration-history DDL; the minimal receipt is left unchanged.
            if matches!(
                fleet_sync_receiver::inspect_copy(scope).await,
                fleet_sync_receiver::CopyInspection::Retired { .. }
            ) {
                return Ok((CopyMigrationResult::Retired, Vec::new()));
            }
            let (applied, effects) = <BootstrapStep as StepCallback<Store, Driver>>::run(
                BootstrapStep {
                    schema: EngineSchema::Copy,
                    now: self.now,
                    clock: self.clock,
                },
                scope,
            )
            .await?;
            let progress = applied.primary;
            let corpus = EngineSchema::Copy.corpus().to_owned();
            let result = if progress.count == 0 {
                CopyMigrationResult::Current {
                    corpus,
                    version: progress.to,
                }
            } else {
                CopyMigrationResult::Migrated {
                    corpus,
                    from: progress.from,
                    to: progress.to,
                }
            };
            Ok((result, effects))
        })
    }
}

#[cfg(test)]
#[path = "fleet_directory_projection_test.rs"]
mod directory_projection_tests;

#[cfg(any(test, feature = "conformance"))]
pub(crate) struct FixedBootstrapClock(pub runtime_primitives::SyncTime);
#[cfg(any(test, feature = "conformance"))]
impl Clock for FixedBootstrapClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        self.0
    }
}
