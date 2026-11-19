use central_storage::{
    AlarmPublicationFailure, AlarmUpdate, BatchOutcome, EngineAlarmDecider, ReadOutcome, Statement,
    StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use fleet_runtime::{EngineCommitFaultKind, TestFaults};
use fleet_runtime_ports::{CentralStore, Clock, EngineTime, SyncTime, WorkspaceStores};
use fleet_store_sqlx::{SqliteObjectStore, StoreOpenFailure};
use sqlx::migrate::Migrator;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};
use workspace_ids::WorkspaceId;

pub struct InMemoryCentralStore {
    store: SqliteObjectStore,
    engine_alarm: Option<(Arc<dyn Clock>, EngineAlarmDecider)>,
    batch_order: tokio::sync::Mutex<()>,
    alarm: Mutex<Option<SyncTime>>,
    faults: TestFaults,
}

impl InMemoryCentralStore {
    pub async fn open(
        migrator: &Migrator,
        clock: Arc<dyn Clock>,
        decide_alarm: EngineAlarmDecider,
    ) -> Result<Self, StoreOpenFailure> {
        Self::open_store(migrator, Some((clock, decide_alarm))).await
    }

    async fn open_store(
        migrator: &Migrator,
        engine_alarm: Option<(Arc<dyn Clock>, EngineAlarmDecider)>,
    ) -> Result<Self, StoreOpenFailure> {
        Ok(Self {
            store: SqliteObjectStore::open_in_memory(migrator).await?,
            engine_alarm,
            batch_order: tokio::sync::Mutex::new(()),
            alarm: Mutex::new(None),
            faults: TestFaults::default(),
        })
    }

    pub fn faults(&self) -> &TestFaults {
        &self.faults
    }
}

impl CentralStore for InMemoryCentralStore {
    async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        let _order = self.batch_order.lock().await;
        let fault = self.faults.take_commit(&batch);
        if let Some(outcome) = fault.as_ref().and_then(|fault| fault.before()) {
            return outcome;
        }
        if self.engine_alarm.is_none()
            && matches!(batch.alarm, AlarmUpdate::SetEngineFromQuery { .. })
        {
            return Err(StoreFailure::AlarmUnpublishable {
                reason: AlarmPublicationFailure::ClockUnavailable,
            });
        }
        if let Some(failure) = self.faults.take_alarm_batch_failure(&batch) {
            return Err(failure);
        }
        let (mut outcome, alarm) = self.store.run_batch(batch).await?;
        if let BatchOutcome::Applied { publication, .. } = &mut outcome {
            let engine = self
                .engine_alarm
                .as_ref()
                .map(|(clock, decide)| (clock.as_ref(), decide));
            *publication = alarm.publish(engine, |at| {
                *self.alarm.lock().unwrap_or_else(PoisonError::into_inner) = at;
            });
            if fault.is_some_and(|fault| fault.kind == EngineCommitFaultKind::AppliedAnswerLost) {
                return Err(StoreFailure::AnswerLost);
            }
        }
        Ok(outcome)
    }

    async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        if let Some(failure) = self.faults.take_read_failure(&statement) {
            return Err(failure);
        }
        self.store.read(statement).await
    }

    async fn published_alarm(&self) -> Result<Option<EngineTime>, StoreFailure> {
        self.alarm
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map(|at| {
                EngineTime::from_timestamp_millis(at.unix_millis()).ok_or_else(|| {
                    fleet_runtime_ports::workspace_decode_error::corrupt("alarm.deadline")
                })
            })
            .transpose()
    }
}

#[derive(Clone, Default)]
pub struct InMemoryWorkspaceStores(BTreeMap<WorkspaceId, Arc<InMemoryCentralStore>>);

impl InMemoryWorkspaceStores {
    pub async fn open_workspace(
        &mut self,
        workspace: WorkspaceId,
        migrator: &Migrator,
    ) -> Result<(), StoreOpenFailure> {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.0.entry(workspace) {
            entry.insert(Arc::new(
                InMemoryCentralStore::open_store(migrator, None).await?,
            ));
        }
        Ok(())
    }
}

impl WorkspaceStores for InMemoryWorkspaceStores {
    async fn call_workspace(
        &self,
        workspace: &WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        let store = self.0.get(workspace).ok_or(StoreFailure::Unavailable)?;
        match call {
            StoreCall::Read(statement) => store.read(statement).await.map(StoreReply::Read),
            StoreCall::Batch(batch) => store.run_batch(batch).await.map(StoreReply::Batch),
        }
    }
}
