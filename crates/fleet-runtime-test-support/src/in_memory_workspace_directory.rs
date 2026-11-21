use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, Statement, StatementBatch,
    StoreFailure,
};
use fleet_contracts::{DirectoryWorkspace, WorkspaceDirectory, WorkspaceView};
use fleet_runtime_ports::{CentralStore, EngineTime};
use fleet_storage_schema::workspace_migrations::central_migrator;
use fleet_store_sqlx::{SqliteObjectStore, StoreOpenFailure};
use fleet_workspace_directory::CentralWorkspaceDirectory;
use std::path::Path;
use workspace_model::{Description, ResourceName, Timestamp, WorkspaceId};

/// WorkspaceDirectory fixture backed by one organization's migrated in-memory SQLite store.
pub struct InMemoryWorkspaceDirectory {
    store: InMemoryDirectoryStore,
}

impl InMemoryWorkspaceDirectory {
    pub async fn open() -> Result<Self, StoreOpenFailure> {
        let sqlite =
            SqliteObjectStore::open_and_migrate(Path::new(":memory:"), &central_migrator()).await?;
        Ok(Self {
            store: InMemoryDirectoryStore(sqlite),
        })
    }

    /// Applies a planned insert as the fixture's caller.
    pub async fn apply_guarded_insert(
        &self,
        insert: GuardedStatement,
    ) -> Result<BatchOutcome, StoreFailure> {
        self.store
            .run_batch(StatementBatch {
                statements: vec![insert],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
    }
}

impl WorkspaceDirectory for InMemoryWorkspaceDirectory {
    async fn read_by_id(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
        CentralWorkspaceDirectory::new(&self.store)
            .read_by_id(workspace_id)
            .await
    }

    async fn read_by_name(
        &self,
        name: &ResourceName,
    ) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
        CentralWorkspaceDirectory::new(&self.store)
            .read_by_name(name)
            .await
    }

    fn plan_guarded_insert(
        &self,
        workspace_id: &WorkspaceId,
        name: &ResourceName,
        description: &Description,
        now: Timestamp,
    ) -> Result<GuardedStatement, StoreFailure> {
        CentralWorkspaceDirectory::new(&self.store).plan_guarded_insert(
            workspace_id,
            name,
            description,
            now,
        )
    }

    fn workspace_view(&self, row: DirectoryWorkspace) -> Result<WorkspaceView, StoreFailure> {
        CentralWorkspaceDirectory::new(&self.store).workspace_view(row)
    }
}

struct InMemoryDirectoryStore(SqliteObjectStore);

impl CentralStore for InMemoryDirectoryStore {
    async fn run_batch(&self, batch: StatementBatch) -> Result<BatchOutcome, StoreFailure> {
        assert!(matches!(batch.alarm, AlarmUpdate::Unchanged));
        self.0.run_batch(batch).await.map(|(outcome, _)| outcome)
    }

    async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        self.0.read(statement).await
    }

    async fn published_alarm(&self) -> Result<Option<EngineTime>, StoreFailure> {
        Ok(None)
    }
}
