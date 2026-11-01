//! Database connection lifecycle; schema setup precedes every repository operation.
use crate::{StorageError, central_firing_receipts, central_report_outbox};
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

pub struct AutomationStore {
    pub(crate) connection: SqliteConnection,
    #[cfg(debug_assertions)]
    pub(crate) automation_faults: Option<std::sync::Arc<dyn crate::AutomationFaultObserver>>,
    pub(crate) central_reports_changed: std::sync::Arc<tokio::sync::Notify>,
    pub(crate) central_report_clock: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl AutomationStore {
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        let mut connection = connect(path, true).await?;
        crate::schema_initialization::initialize(&mut connection).await?;
        Ok(Self {
            connection,
            #[cfg(debug_assertions)]
            automation_faults: None,
            central_reports_changed: std::sync::Arc::new(tokio::sync::Notify::new()),
            central_report_clock: std::sync::Arc::new(|| chrono::Utc::now().timestamp_millis()),
        })
    }
    #[cfg(debug_assertions)]
    pub fn set_automation_faults(
        &mut self,
        faults: std::sync::Arc<dyn crate::AutomationFaultObserver>,
    ) {
        self.automation_faults = Some(faults);
    }
    #[cfg(debug_assertions)]
    pub fn automation_faults(&self) -> Option<std::sync::Arc<dyn crate::AutomationFaultObserver>> {
        self.automation_faults.clone()
    }
    pub fn central_report_changes(&self) -> std::sync::Arc<tokio::sync::Notify> {
        std::sync::Arc::clone(&self.central_reports_changed)
    }
    pub fn set_central_report_clock(
        &mut self,
        clock: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
    ) {
        self.central_report_clock = clock;
    }
    pub async fn central_report_batch(
        &mut self,
        now_ms: i64,
    ) -> Result<crate::CentralReportBatch, StorageError> {
        central_report_outbox::central_report_batch(&mut self.connection, now_ms).await
    }
    pub async fn acknowledge_central_reports(
        &mut self,
        accepted: &[collaboration_protocol::AcceptedWorkReport],
    ) -> Result<(), StorageError> {
        central_report_outbox::acknowledge_central_reports(&mut self.connection, accepted).await
    }
    pub async fn read_central_firing(
        &mut self,
        firing_id: &collaboration_protocol::FiringId,
    ) -> Result<Option<crate::CentralFiringReceipt>, StorageError> {
        central_firing_receipts::read_receipt(&mut self.connection, firing_id).await
    }
    pub async fn admit_central_firing(
        &mut self,
        request: &crate::CentralFiringRequest,
    ) -> Result<crate::CentralFiringReceipt, StorageError> {
        central_firing_receipts::admit_central_firing(
            &mut self.connection,
            request,
            &self.central_reports_changed,
            #[cfg(debug_assertions)]
            self.automation_faults.as_deref(),
        )
        .await
    }
    #[cfg(debug_assertions)]
    pub async fn central_run_fault_evidence(
        &mut self,
        run_id: &agent_automation::RunId,
    ) -> Result<Option<crate::AutomationFaultEvidence>, StorageError> {
        central_firing_receipts::central_run_fault_evidence(&mut self.connection, run_id).await
    }
    #[cfg(debug_assertions)]
    pub async fn central_delivery_fault_evidence(
        &mut self,
        delivery_id: &agent_automation::DeliveryId,
    ) -> Result<Option<crate::AutomationFaultEvidence>, StorageError> {
        central_firing_receipts::central_delivery_fault_evidence(&mut self.connection, delivery_id)
            .await
    }
    pub async fn close(self) -> Result<(), StorageError> {
        self.connection.close().await?;
        Ok(())
    }
}

/// Applies the message drop after the upgrade has committed and verified its copy.
///
/// The caller must stop all file owners and verify counts and per-row digests
/// across the source and destination first. Pre-release mail state always refuses.
pub async fn apply_message_drop_migration(path: &Path) -> Result<(), StorageError> {
    let mut connection = connect(path, false).await?;
    let initialized =
        crate::schema_initialization::initialize_after_verified_message_copy(&mut connection).await;
    let closed = connection.close().await.map_err(StorageError::from);
    initialized.and(closed)
}

async fn connect(path: &Path, create_if_missing: bool) -> Result<SqliteConnection, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create_if_missing)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Delete)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(1));
    Ok(SqliteConnection::connect_with(&options).await?)
}
