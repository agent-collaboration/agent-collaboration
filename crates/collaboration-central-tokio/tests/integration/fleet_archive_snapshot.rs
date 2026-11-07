//! Exact persisted archive state shared by the real hook and settlement rollback cases.
use super::composed_fleet_fixture::*;
use super::typed_claim_observer as claim_observer;
use collaboration_central_core::ClaimReservationState;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::Record;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

#[derive(Debug, PartialEq)]
pub(super) struct ArchiveState {
    pub records: Vec<Record>,
    pub feed: Vec<(i64, String)>,
    pub ledgers: Vec<String>,
    pub reports: Vec<Value>,
    pub projection: Option<String>,
    pub reservation: Option<ClaimReservationState>,
    pub registry: Option<fleet_sync_lease::RegistrySnapshot>,
    pub incarnations: Vec<i64>,
    pub notices: Vec<String>,
}
pub(super) async fn inspect(
    fixture: &Fixture,
    attempt: durable_steps::Uuid,
) -> TestResult<ArchiveState> {
    Ok(fixture.store.transaction(move |scope|Box::pin(async move {
        let records = crate::fleet_record_rows::metadata_records(scope).await?;
        let feed = crate::fleet_record_rows::feed_entries(scope).await?;
        let ledgers = crate::fleet_ledger_rows::in_scope(scope).await?;
        let reports = query::<sqlx_turso::Turso>("SELECT report_id,board_id,project_id,epoch,commit_sequence,installation,reported_at,created_at,updated_at FROM fleet_archive_reports ORDER BY board_id,project_id").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok::<_,StoreError>(json!({
            "report_id":row.try_get::<String,_>("report_id").map_err(StoreError::backend)?,
            "board_id":row.try_get::<String,_>("board_id").map_err(StoreError::backend)?,
            "project_id":row.try_get::<String,_>("project_id").map_err(StoreError::backend)?,
            "epoch":row.try_get::<String,_>("epoch").map_err(StoreError::backend)?,
            "commit_sequence":row.try_get::<String,_>("commit_sequence").map_err(StoreError::backend)?,
            "installation":row.try_get::<String,_>("installation").map_err(StoreError::backend)?,
            "reported_at":row.try_get::<String,_>("reported_at").map_err(StoreError::backend)?,
            "created_at":row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?,
            "updated_at":row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?,
        }))).collect::<Result<Vec<_>,_>>()?;
        let projection = collaboration_central_core::read_fleet_lease_projection_in_scope(scope,PROJECT.parse().map_err(StoreError::backend)?).await?;
        let row = query::<sqlx_turso::Turso>("SELECT d.project_id,r.state_kind,r.outcome FROM fleet_project_directory d LEFT JOIN lease_claim_reservations r ON r.project_id=d.project_id AND r.attempt_id=? WHERE d.project_id=?").bind(attempt.to_string()).bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let reservation = claim_observer::reservation_state(&row)?.map(|value|serde_json::from_value(value).map_err(StoreError::backend)).transpose()?;
        let registry = fleet_sync_lease::registry_snapshot(scope,PROJECT.to_owned().try_into().map_err(StoreError::backend)?)
            .await.map_err(StoreError::backend)?;
        let incarnations = query::<sqlx_turso::Turso>("SELECT incarnation FROM lease_incarnation ORDER BY name")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter()
            .map(|row|row.try_get("incarnation").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
        let notices = query::<sqlx_turso::Turso>("SELECT notice_id FROM machine_control_project_notices ORDER BY notice_id")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter()
            .map(|row|row.try_get("notice_id").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
        Ok(ArchiveState { records, feed, ledgers, reports, projection,reservation,
            registry,incarnations,notices })
    })).await?)
}
