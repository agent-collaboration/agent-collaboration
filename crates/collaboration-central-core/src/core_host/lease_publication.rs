//! Discover current public snapshots; only outstanding work arms another wake.
use super::lease_settlement::RegistryLeasePublication;
use super::{CoreHost, PlatformIo, SharedTaskCaller};
use crate::{
    fleet_lease_calls::PublishLeaseProjection,
    lease_projection::{LeaseProjection, LeaseProjectionResult},
    registry_lease_store::LeaseSnapshotRead,
};
use durable_steps::StoreError;
use std::time::Duration;

impl<Io: PlatformIo> CoreHost<Io> {
    pub(crate) async fn retirement_directory_acknowledged(
        &self,
        database: fleet_sync_protocol::DatabaseId,
        retired_at: fleet_sync_protocol::SyncTime,
    ) -> Result<bool, StoreError> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Err(StoreError::backend(
                "Registry has no lease publication owner",
            ));
        };
        let LeaseSnapshotRead::Available { snapshot, holder } =
            publication.store.snapshot(database).await?
        else {
            return Err(StoreError::backend(
                "retired Registry snapshot is unreadable",
            ));
        };
        if snapshot.lifecycle != (fleet_sync_lease::DatabaseLifecycle::Retired { at: retired_at }) {
            return Err(StoreError::Corrupt {
                field: "retirement.snapshot".into(),
                message: "retirement result differs from the public Registry lifecycle".into(),
            });
        }
        let Some(incarnation) = publication.run.current().map_err(|_| run_unavailable())? else {
            return Ok(false);
        };
        let version = LeaseProjection::from_snapshot(snapshot, holder, incarnation)
            .map_err(StoreError::backend)?
            .version();
        let acknowledged = publication
            .store
            .acknowledged(database)
            .await?
            .is_some_and(|handled| handled >= version);
        // All awaited reads finish before this check; no run guard spans I/O.
        Ok(acknowledged
            && publication.run.current().map_err(|_| run_unavailable())? == Some(incarnation))
    }
    pub(crate) async fn initialize_lease_publications(&self) -> Result<(), StoreError> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Ok(());
        };
        let Some(discovery) = publication.startup.begin()? else {
            return Ok(());
        };
        self.request_lease_publication_repair().await?;
        discovery.complete()
    }
    pub(crate) async fn request_lease_publication_repair(&self) -> Result<(), StoreError> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Ok(());
        };
        if publication.store.databases().await?.is_empty() {
            return Ok(());
        }
        if self.lease_publications_pending(publication).await? {
            self.io
                .wake_at(self.io.now())
                .await
                .map_err(|error| StoreError::Aborted {
                    reason: format!("lease publication wake failed: {error:?}"),
                })?;
        }
        Ok(())
    }
    pub(crate) async fn repair_lease_publications(&self) -> Result<(), StoreError> {
        let SharedTaskCaller::Registry {
            lease_publication: Some(publication),
            ..
        } = &self.caller
        else {
            return Ok(());
        };
        let result = self.publish_current_leases(publication).await;
        let pending = match &result {
            Ok(pending) => *pending,
            Err(_) => true,
        };
        if pending {
            self.io
                .wake_at(self.io.now().after(Duration::from_secs(5)))
                .await
                .map_err(|error| StoreError::Aborted {
                    reason: format!("lease publication wake failed: {error:?}"),
                })?;
        }
        result.map(|_| ())
    }
    async fn publish_current_leases(
        &self,
        publication: &RegistryLeasePublication,
    ) -> Result<bool, StoreError> {
        let databases = publication.store.databases().await?;
        if databases.is_empty() {
            return Ok(false);
        }
        // Notice intent is local to Registry and never waits for Fleet publication.
        // Native journal admission is durable, but its platform timer is released
        // only after this StepDriver transaction has committed.
        for database in &databases {
            if publication
                .store
                .admit_notices(*database, self.io.now())
                .await?
            {
                self.io
                    .wake_at(self.io.now())
                    .await
                    .map_err(StoreError::backend)?;
            }
        }
        let incarnation = match self.acquire_registry_incarnation(publication).await {
            Ok(incarnation) => incarnation,
            Err(_) => return Ok(true),
        };
        for database in databases {
            let LeaseSnapshotRead::Available { snapshot, holder } =
                publication.store.snapshot(database).await?
            else {
                continue;
            };
            let projection = LeaseProjection::from_snapshot(snapshot, holder, incarnation)
                .map_err(StoreError::backend)?;
            let version = projection.version();
            if publication
                .store
                .acknowledged(database)
                .await?
                .is_some_and(|handled| handled >= version)
            {
                continue;
            }
            let identity =
                self.call_identity("lease_projection")
                    .map_err(|_| StoreError::Aborted {
                        reason: "runtime could not provide a lease publication identity".into(),
                    })?;
            match self
                .registry_fleet_call(PublishLeaseProjection, projection, identity)
                .await
            {
                Ok(
                    LeaseProjectionResult::Applied
                    | LeaseProjectionResult::Duplicate
                    | LeaseProjectionResult::Older { .. },
                ) => {
                    publication
                        .store
                        .acknowledge(database, version, self.io.new_id())
                        .await?;
                }
                Ok(LeaseProjectionResult::Fenced { fence }) => {
                    publication
                        .run
                        .invalidate(fence.value())
                        .map_err(|_| run_unavailable())?;
                }
                Ok(LeaseProjectionResult::Conflict { stored }) => {
                    publication
                        .run
                        .invalidate(stored.incarnation().value())
                        .map_err(|_| run_unavailable())?;
                }
                Err(_) => {}
            }
        }
        self.lease_publications_pending(publication).await
    }
    async fn lease_publications_pending(
        &self,
        publication: &RegistryLeasePublication,
    ) -> Result<bool, StoreError> {
        let Some(incarnation) = publication.run.current().map_err(|_| run_unavailable())? else {
            return Ok(true);
        };
        for database in publication.store.databases().await? {
            if publication.store.notices_pending(database).await? {
                return Ok(true);
            }
            let LeaseSnapshotRead::Available { snapshot, holder } =
                publication.store.snapshot(database).await?
            else {
                return Ok(true);
            };
            let current = LeaseProjection::from_snapshot(snapshot, holder, incarnation)
                .map_err(StoreError::backend)?
                .version();
            if publication
                .store
                .acknowledged(database)
                .await?
                .is_none_or(|handled| handled < current)
            {
                return Ok(true);
            }
        }
        // A concurrent response can invalidate or replace the run while the reads await.
        Ok(publication.run.current().map_err(|_| run_unavailable())? != Some(incarnation))
    }
}
fn run_unavailable() -> StoreError {
    StoreError::Aborted {
        reason: "Registry publication run is unavailable".into(),
    }
}
