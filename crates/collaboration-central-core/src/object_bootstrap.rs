//! Apply library-owned corpora once, in central's one owned store transaction.

use crate::{
    ObjectHost, generated_bootstrap,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, executor::Executor, types::Type,
};
use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

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
    Mailbox,
    Fleet,
}
impl EngineSchema {
    fn corpus(self) -> &'static str {
        match self {
            Self::Registry => "fleet-sync-lease",
            Self::Copy => "fleet-sync-receiver",
            Self::Session => "fleet-sync-session",
            Self::Mailbox => "fleet-sync-delivery",
            Self::Fleet => "central-fleet",
        }
    }
    fn migrations(self) -> Vec<&'static sqlx_core::migrate::Migration> {
        match self {
            Self::Registry => fleet_sync_lease::LEASE_MIGRATOR.iter().collect(),
            Self::Copy => fleet_sync_receiver::RECEIVER_MIGRATOR.iter().collect(),
            Self::Session => fleet_sync_session::SESSION_MIGRATOR.iter().collect(),
            Self::Mailbox => fleet_sync_delivery::DELIVERY_MIGRATOR.iter().collect(),
            Self::Fleet => FLEET_MIGRATIONS.iter().collect(),
        }
    }
}

pub struct ObjectBootstrap<Host: ObjectHost> {
    driver: StepDriver<Host>,
    now: runtime_primitives::SyncTime,
}
impl<Host: ObjectHost> ObjectBootstrap<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn new(host: Arc<Host>, now: runtime_primitives::SyncTime) -> Self {
        Self {
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
            })
            .await?;
        Ok(applied.count)
    }

    pub(crate) async fn migrate_copy(&self) -> crate::owner_operation_calls::CopyMigrationResult {
        use crate::owner_operation_calls::CopyMigrationResult;
        match self.driver.run(MigrateCopyStep { now: self.now }).await {
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
                    reason: error.to_string(),
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
    schema: EngineSchema,
    now: runtime_primitives::SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for BootstrapStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
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
            )
            .await?;
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
                1,
                "machine notices".into(),
                MigrationType::Simple,
                include_str!("../session_migrations/0001_machine_notices.sql").into_sql_str(),
                false,
            ),
            Migration::new(
                2,
                "forward correlations".into(),
                MigrationType::Simple,
                include_str!("../session_migrations/0002_forward_correlations.sql").into_sql_str(),
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
            (
                3,
                "lease publication acknowledgements",
                include_str!("../registry_migrations/0003_lease_acknowledgements.sql"),
            ),
            (
                4,
                "registry notice marks",
                include_str!("../registry_migrations/0004_notice_marks.sql"),
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
) -> Result<CorpusProgress, StoreError>
where
    Scope: TransactionScope,
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
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
        crate::fleet_record_codec::check_private_id(
            &row.row_id
                .ok_or_else(|| corrupt("missing physical row identity"))?,
            "migration row identity",
        )?;
        let text = row.version.ok_or_else(|| corrupt("missing version"))?;
        let version = text
            .parse::<i64>()
            .map_err(|_| corrupt("invalid version"))?;
        if version.to_string() != text {
            return Err(corrupt("noncanonical version"));
        }
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
        if &checksum(migration)? != saved {
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
        generated_bootstrap::insert_migration_history(
            scope,
            durable_steps::Uuid::now_v7().to_string(),
            corpus.into(),
            migration.version.to_string(),
            checksum(migration)?,
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

fn checksum(migration: &sqlx_core::migrate::Migration) -> Result<String, StoreError> {
    serde_json::to_string(migration.checksum.as_ref()).map_err(StoreError::backend)
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
    now: runtime_primitives::SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for MigrateCopyStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
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
