//! Single storage owner with commit-triggered journal reads and bounded maintenance.
use crate::{JournalError, JournalPage, JournalPosition, JournalRow, ObservationJournal};
use collaboration_protocol::{EndpointRef, LifecycleObservation};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, watch};

pub struct LifecycleStore {
    journal: Mutex<ObservationJournal>,
    commits: watch::Sender<u64>,
    coverage: std::sync::Mutex<crate::ObservationCoverage>,
    snapshots: Mutex<crate::AddressSnapshotCache>,
    available: AtomicBool,
}

impl LifecycleStore {
    #[must_use]
    pub fn new(journal: ObservationJournal) -> Self {
        let (commits, _) = watch::channel(0);
        Self {
            journal: Mutex::new(journal),
            commits,
            coverage: std::sync::Mutex::new(crate::ObservationCoverage::default()),
            snapshots: Mutex::new(crate::AddressSnapshotCache::default()),
            available: AtomicBool::new(true),
        }
    }
    pub async fn bounds(&self) -> Result<crate::JournalBounds, JournalError> {
        let mut journal = self.journal.lock().await;
        self.require_available()?;
        let result = journal.bounds().await;
        self.observe_storage_result(&result)?;
        result
    }
    pub async fn append(
        &self,
        observation: &LifecycleObservation,
        now: i64,
    ) -> Result<JournalRow, JournalError> {
        observation
            .validate()
            .map_err(|_| JournalError::InvalidRecord)?;
        if now < 0 {
            return Err(JournalError::InvalidRecord);
        }
        let mut journal = self.journal.lock().await;
        self.require_available()?;
        let appended = match journal.append(observation, now).await {
            Err(JournalError::Capacity) => {
                // A failed append rolls back before maintenance. Reclaim only the
                // expired prefix, then make one fresh metadata admission attempt.
                match journal.expire_history(now).await {
                    Ok(_) => journal.append(observation, now).await,
                    Err(error) => Err(error),
                }
            }
            result => result,
        };
        let row = match appended {
            Ok(row) => row,
            Err(error) => {
                self.invalidate_storage()?;
                return Err(error);
            }
        };
        self.coverage
            .lock()
            .map_err(|_| JournalError::InvalidStorage)?
            .apply(observation)?;
        self.signal_commit();
        Ok(row)
    }
    /// A receiver that wakes after each journal append or maintenance that removed rows,
    /// including a native `turn/completed` observation.
    #[must_use]
    pub fn subscribe_commits(&self) -> watch::Receiver<u64> {
        self.commits.subscribe()
    }
    fn signal_commit(&self) {
        self.commits
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    fn require_available(&self) -> Result<(), JournalError> {
        if self.available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(JournalError::InvalidStorage)
        }
    }
    fn invalidate_storage(&self) -> Result<(), JournalError> {
        self.available.store(false, Ordering::Release);
        self.coverage
            .lock()
            .map_err(|_| JournalError::InvalidStorage)?
            .storage_unavailable();
        self.signal_commit();
        Ok(())
    }
    fn observe_storage_result<TRecord>(
        &self,
        result: &Result<TRecord, JournalError>,
    ) -> Result<(), JournalError> {
        if matches!(
            result,
            Err(JournalError::Storage(_) | JournalError::InvalidStorage | JournalError::Capacity)
        ) {
            self.invalidate_storage()?;
        }
        Ok(())
    }
    async fn read_page(
        &self,
        endpoint: &EndpointRef,
        after: JournalPosition,
        page_size: u32,
    ) -> Result<JournalPage, JournalError> {
        let mut journal = self.journal.lock().await;
        self.require_available()?;
        let result = journal.read_page(endpoint, after, page_size).await;
        self.observe_storage_result(&result)?;
        result
    }
    pub async fn read_wait(
        &self,
        endpoint: &EndpointRef,
        after: JournalPosition,
        page_size: u32,
        wait_milliseconds: u64,
    ) -> Result<JournalPage, JournalError> {
        if wait_milliseconds > 30000 {
            return Err(JournalError::InvalidRecord);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_milliseconds);
        // Subscribe before the first read to avoid missing a commit between read and wait.
        let mut wake = self.commits.subscribe();
        let mut position = after;
        loop {
            let page = self
                .read_page(endpoint, position.clone(), page_size)
                .await?;
            if !page.records.is_empty()
                || !page.caught_up
                || wait_milliseconds == 0
                || tokio::time::Instant::now() >= deadline
            {
                return Ok(page);
            }
            position = page.next.clone();
            if tokio::time::timeout_at(deadline, wake.changed())
                .await
                .is_err()
            {
                return self.read_page(endpoint, position, page_size).await;
            }
        }
    }
    /// Completes startup maintenance before the owner admits public journal reads.
    pub async fn prepare(&self, now: i64) -> Result<(), JournalError> {
        let mut journal = self.journal.lock().await;
        self.invalidate_storage()?;
        journal.rebuild_addresses().await?;
        journal.expire_history(now).await?;
        journal.incremental_vacuum_step().await?;
        *self
            .coverage
            .lock()
            .map_err(|_| JournalError::InvalidStorage)? = crate::ObservationCoverage::default();
        *self.snapshots.lock().await = crate::AddressSnapshotCache::default();
        self.available.store(true, Ordering::Release);
        self.signal_commit();
        Ok(())
    }
    /// Expires old history; it signals journal readers only when it removed rows.
    pub async fn maintain(&self, now: i64) -> Result<(), JournalError> {
        self.expire_history_reporting_removal(now).await.map(|_| ())
    }
    /// Expires history like [`Self::maintain`] and says whether it removed rows.
    async fn expire_history_reporting_removal(&self, now: i64) -> Result<bool, JournalError> {
        let mut journal = self.journal.lock().await;
        let result = journal.expire_history_reporting_removal(now).await;
        self.observe_storage_result(&result)?;
        let expiry = result?;
        journal.incremental_vacuum_step().await?;
        if expiry.removed_rows {
            self.signal_commit();
        }
        Ok(expiry.removed_rows)
    }
    /// When expiry next removes something, read like any journal read.
    async fn retention_due_ms(&self, now_ms: i64) -> Result<Option<i64>, JournalError> {
        let mut journal = self.journal.lock().await;
        let result = journal.retention_due_ms(now_ms).await;
        self.observe_storage_result(&result)?;
        result
    }
    pub async fn close(self) {
        self.journal.into_inner().close().await;
    }
}

const fn lifecycle_maintenance_error_class(error: &JournalError) -> &'static str {
    match error {
        JournalError::SnapshotExpired => "snapshot_expired",
        JournalError::JournalChanged => "journal_changed",
        JournalError::HistoryExpired => "history_expired",
        JournalError::Storage(_) => "storage",
        JournalError::InvalidRecord => "invalid_record",
        JournalError::InvalidStorage => "invalid_storage",
        JournalError::InvalidCheckpointAddress { .. } => "invalid_checkpoint_address",
        JournalError::Capacity => "capacity",
        JournalError::StorageMaintenance(_) => "storage_maintenance",
    }
}

impl LifecycleStore {
    pub async fn address_snapshot(
        &self,
        endpoint: &EndpointRef,
        page_size: usize,
        cursor: Option<&str>,
        snapshot_id: collaboration_protocol::UuidIdentity,
        captured_at: collaboration_protocol::ObservationTimestamp,
    ) -> Result<crate::AddressPage, JournalError> {
        self.require_available()?;
        if let Some(cursor) = cursor {
            return self.snapshots.lock().await.page(
                endpoint,
                page_size,
                cursor,
                std::time::Instant::now(),
            );
        }
        let mut journal = self.journal.lock().await;
        self.require_available()?;
        let captured = journal.capture_addresses(endpoint).await;
        self.observe_storage_result(&captured)?;
        let captured = captured?;
        let coverage = self
            .coverage
            .lock()
            .map_err(|_| JournalError::InvalidStorage)?
            .view(endpoint, captured_at.clone());
        self.snapshots
            .lock()
            .await
            .insert(crate::AddressSnapshotInputs {
                id: snapshot_id,
                endpoint: endpoint.clone(),
                page_size,
                captured,
                now: std::time::Instant::now(),
                coverage,
                captured_at,
            })
    }
}

#[path = "lifecycle_store/retention_worker.rs"]
mod retention_worker;

#[cfg(test)]
mod tests {
    use collaboration_protocol::LifecycleObservation;
    use std::env;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::LifecycleStore;
    use crate::JournalError;
    use crate::ObservationJournal;

    #[path = "wal_mode_tests.rs"]
    mod wal_mode_tests;

    #[path = "maintenance_signal_tests.rs"]
    mod maintenance_signal_tests;

    #[tokio::test]
    async fn new_journal_database_uses_incremental_auto_vacuum() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let database_path = directory.path().join("session-registry.sqlite");
        let mut journal = ObservationJournal::open(&database_path, identity())
            .await
            .expect("new journal database should open");
        let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&mut journal.connection)
            .await
            .expect("journal auto-vacuum mode should be readable");
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut journal.connection)
            .await
            .expect("journal mode should be readable");
        assert_eq!(mode, 2);
        assert_eq!(journal_mode, "wal");
    }

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn lifecycle_maintenance_error_class_excludes_raw_storage_details() {
        let error = JournalError::Storage(sqlx::Error::Protocol(
            "raw-lifecycle-storage-error-canary".to_owned(),
        ));

        let error_class = super::lifecycle_maintenance_error_class(&error);

        assert_eq!(error_class, "storage");
        assert!(!error_class.contains("raw-lifecycle-storage-error-canary"));
    }

    fn identity() -> collaboration_protocol::UuidIdentity {
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap_or_else(|error| panic!("test identity should parse: {error}"))
    }

    fn observation() -> LifecycleObservation {
        serde_json::from_value(serde_json::json!({
            "observedAt": "2026-09-05T12:00:00Z",
            "source": "observerLifecycle",
            "scope": {
                "endpoint": {
                    "serviceId": "00000000-0000-4000-8000-000000000001",
                    "endpointId": "codex-local"
                },
                "generation": null,
                "observerId": "00000000-0000-4000-8000-000000000002"
            },
            "subject": {"kind": "backend"},
            "change": {"kind": "coverageLost"}
        }))
        .unwrap_or_else(|error| panic!("test observation should parse: {error}"))
    }

    fn test_database_path(name: &str) -> std::path::PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "lifecycle-observation-{name}-{}-{counter}.sqlite",
            std::process::id()
        ))
    }
}
