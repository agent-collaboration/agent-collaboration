//! The named fleet port uses a native in-process stand-in until central lands.
use fleet_admission::{FleetIds, FleetRequest};
use message_board::{Record, RecordRef, RequestResult};
use message_board_storage::{
    FleetCache, FleetOutbox, FleetOutboxEntry, FleetReply, FleetStorageError, FleetStore,
    RouterLocalStorageError,
};
use std::path::Path;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum FleetClientError {
    #[error("fleet directory is unavailable: {0}")]
    Directory(#[from] std::io::Error),
    #[error(transparent)]
    Authoritative(#[from] FleetStorageError),
    #[error(transparent)]
    Local(#[from] RouterLocalStorageError),
    #[error(transparent)]
    Project(#[from] crate::ProjectRegistryError),
}

pub struct FleetClient {
    stand_in: FleetStore,
    outbox: FleetOutbox,
    cache: FleetCache,
    admission_mutex: Mutex<()>,
}

impl FleetClient {
    /// The local producer supplies its own installation; storage owns acknowledgement.
    /// M4 replaces this stand-in with the authenticated session report.
    pub async fn report_archive_seen(
        &self,
        mut report: fleet_admission::ArchiveReport,
    ) -> Result<fleet_admission::ArchiveReportOutcome, FleetClientError> {
        let _guard = self.admission_mutex.lock().await;
        report.reported_at = chrono::Utc::now();
        let outcome = self.stand_in.report_archive_seen(&report).await?;
        self.refresh_cache().await?;
        Ok(outcome)
    }

    pub async fn open_local_stand_in(directory: &Path) -> Result<Self, FleetClientError> {
        tokio::fs::create_dir_all(directory).await?;
        let stand_in = FleetStore::open(&directory.join("fleet.sqlite")).await?;
        let outbox = FleetOutbox::open(&directory.join("fleet-outbox.sqlite")).await?;
        let cache = FleetCache::open(&directory.join("fleet-cache.sqlite")).await?;
        // A crash between admission and projection is repaired from the authoritative snapshot.
        cache
            .replace_snapshot(&stand_in.snapshot_records().await?)
            .await?;
        Ok(Self {
            stand_in,
            outbox,
            cache,
            admission_mutex: Mutex::new(()),
        })
    }

    pub async fn submit(
        &self,
        request: &FleetRequest,
        ids: &FleetIds,
    ) -> Result<FleetReply, FleetClientError> {
        let _guard = self.admission_mutex.lock().await;
        let queued = match self.outbox.enqueue(request, ids).await {
            Ok(queued) => queued,
            Err(RouterLocalStorageError::ConflictingRequest) => {
                return Ok(FleetReply {
                    result: RequestResult::ConflictingRequest,
                    records: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let reply = self
            .stand_in
            .admit(&queued.request, chrono::Utc::now(), &queued.ids)
            .await?;
        self.refresh_cache().await?;
        // Only central may settle the outbox; a local decision must retain its original request/IDs.
        Ok(reply)
    }

    async fn refresh_cache(&self) -> Result<(), FleetClientError> {
        let mut after = self.cache.checkpoint().await?;
        loop {
            let page = self.stand_in.feed_after(after, 100).await?;
            self.cache.apply_feed(after, &page.entries).await?;
            after = page.covered_through;
            if after == page.high_water {
                break;
            }
        }
        Ok(())
    }

    pub async fn record(&self, reference: &RecordRef) -> Result<Option<Record>, FleetClientError> {
        Ok(self.cache.record(reference).await?)
    }

    pub async fn process_definition(
        &self,
        reference: &message_board::ProcessDefinitionRef,
    ) -> Result<Option<message_board::ProcessDefinition>, FleetClientError> {
        Ok(self.cache.process_definition(reference).await?)
    }

    pub async fn records(&self) -> Result<Vec<Record>, FleetClientError> {
        Ok(self.cache.snapshot().await?.records)
    }

    pub async fn read_snapshot(
        &self,
    ) -> Result<message_board_storage::FleetCacheSnapshot, FleetClientError> {
        Ok(self.cache.read_snapshot().await?)
    }

    pub async fn workspace_projects(
        &self,
        workspace: &message_board::WorkspaceId,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut projects: std::collections::BTreeSet<_> = self
            .cache
            .workspace_project_ids(workspace)
            .await?
            .into_iter()
            .collect();
        projects.extend(self.outbox.pending_project_ids(workspace).await?);
        Self::without_rejected(projects, registry).await
    }

    pub async fn scoped_projects(
        &self,
        board: Option<&message_board::BoardId>,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut projects: std::collections::BTreeSet<_> = match board {
            Some(board) => self.cache.board_project_ids(board).await?,
            None => self.cache.project_ids().await?,
        }
        .into_iter()
        .collect();
        projects.extend(self.outbox.pending_scoped_projects(board).await?);
        Self::without_rejected(projects, registry).await
    }
    pub async fn pending_workspace_projects(
        &self,
        workspace: &message_board::WorkspaceId,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        Self::without_rejected(
            self.outbox
                .pending_project_ids(workspace)
                .await?
                .into_iter()
                .collect(),
            registry,
        )
        .await
    }
    async fn without_rejected(
        projects: std::collections::BTreeSet<message_board::ProjectId>,
        registry: &crate::ProjectStoreRegistry,
    ) -> Result<Vec<message_board::ProjectId>, FleetClientError> {
        let mut selected = Vec::new();
        for project in projects {
            if !registry.is_rejected_project(&project).await? {
                selected.push(project);
            }
        }
        Ok(selected)
    }
    pub async fn pending_request(
        &self,
        request: &message_board::RequestId,
    ) -> Result<Option<FleetOutboxEntry>, FleetClientError> {
        Ok(self.outbox.pending_request(request).await?)
    }
    pub async fn workspace_page_keys(
        &self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<message_board::WorkspaceId>, FleetClientError> {
        Ok(self.cache.workspace_page_keys(after, limit).await?)
    }
    pub async fn pending_requests(&self) -> Result<Vec<FleetOutboxEntry>, FleetClientError> {
        Ok(self.outbox.pending().await?)
    }
    pub async fn close(self) {
        self.cache.close().await;
        self.outbox.close().await;
        self.stand_in.close().await;
    }
}
