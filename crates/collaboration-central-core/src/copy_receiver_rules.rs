//! Board annotations become local copy state and durable post-commit work.

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, HookRefusal, HookRefusalCode, ReplicationPosition};
use fleet_sync_receiver::{AppliedPiece, HookError, InstalledSnapshot, ReceiverRules};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

/// The summary is board-owned; central adds only its publication position and time.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyDirectorySummary {
    pub database: DatabaseId,
    pub summary: BoardAnnotation,
    pub position: ReplicationPosition,
    pub acked_at: SyncTime,
}

/// Admin seed publication supplies its final annotation as owned input.
/// Ordinary writer seeds pass None and publish no invented summary.
pub struct CopyReceiverRules {
    now: SyncTime,
    snapshot_summary: Option<BoardAnnotation>,
    refusal_code: HookRefusalCode,
}
impl CopyReceiverRules {
    pub fn new(
        now: SyncTime,
        snapshot_summary: Option<BoardAnnotation>,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            now,
            snapshot_summary,
            refusal_code: HookRefusalCode::new("copy_summary_storage")
                .map_err(StoreError::backend)?,
        })
    }
    fn refused(&self, error: impl std::fmt::Display) -> HookError {
        HookError::Refused(HookRefusal {
            code: self.refusal_code.clone(),
            detail: error.to_string(),
        })
    }
    async fn record<S>(
        &self,
        scope: &mut S,
        database: DatabaseId,
        position: ReplicationPosition,
        summary: &BoardAnnotation,
    ) -> Result<Vec<CopyDirectorySummary>, HookError>
    where
        S: TransactionScope,
        <S::Database as Database>::Arguments: IntoArguments<S::Database>,
        for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    {
        let summary = CopyDirectorySummary {
            database,
            summary: summary.clone(),
            position,
            acked_at: self.now,
        };
        crate::generated_copy_summary::store_copy_summary(
            scope,
            database.as_uuid().to_string(),
            serde_json::to_string(&summary).map_err(|error| self.refused(error))?,
        )
        .await
        .map_err(fleet_sync_receiver::classify_sqlx_error)?;
        Ok(vec![summary])
    }
}
impl<S> ReceiverRules<S> for CopyReceiverRules
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
{
    type Protocol = BoardConsumerProtocol;
    type FollowUp = CopyDirectorySummary;
    fn local_migrations(&self) -> &'static [sqlx_core::migrate::Migration] {
        &COPY_MIGRATIONS
    }
    async fn on_applied(
        &self,
        scope: &mut S,
        applied: &AppliedPiece<'_, BoardAnnotation>,
    ) -> Result<Vec<CopyDirectorySummary>, HookError> {
        match &applied.annotation {
            Some(summary) => {
                self.record(scope, applied.database, applied.through, summary)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }
    async fn on_snapshot_installed(
        &self,
        scope: &mut S,
        installed: &InstalledSnapshot,
    ) -> Result<Vec<CopyDirectorySummary>, HookError> {
        match &self.snapshot_summary {
            Some(summary) => {
                self.record(scope, installed.database, installed.position, summary)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }
}
pub(crate) const COPY_SUMMARY_CORPUS: &str = "central-copy-summary";
pub(crate) const COPY_SUMMARY_VERSION: i64 = 1;

static COPY_MIGRATIONS: std::sync::LazyLock<Vec<sqlx_core::migrate::Migration>> =
    std::sync::LazyLock::new(|| {
        use sqlx_core::{
            migrate::{Migration, MigrationType},
            sql_str::SqlSafeStr,
        };
        vec![Migration::new(
            COPY_SUMMARY_VERSION,
            "copy summary".into(),
            MigrationType::Simple,
            include_str!("../copy_migrations/0001_copy_summary.sql").into_sql_str(),
            false,
        )]
    });
