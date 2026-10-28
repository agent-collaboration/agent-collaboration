//! Cache transaction and reader cases now take their inputs from native central.
#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_source;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/fleet_cache_source.rs"]
mod source;
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{FleetCache, FleetRecordSnapshot, RouterLocalStorageError};
use source::NativeFleetSource;
use sqlx_core::{
    executor::Executor,
    pool::{Pool, PoolOptions},
};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::{path::PathBuf, sync::Arc, time::Duration};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    source: NativeFleetSource,
    observer: Pool<Turso>,
    cache: Arc<FleetCache>,
    request: FleetRequest,
    original: FleetRecordSnapshot,
}
impl Fixture {
    async fn open() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let source = NativeFleetSource::open(&directory.path().join("source")).await?;
        let path = directory.path().join("cache.sqlite");
        let cache = Arc::new(FleetCache::open(&path).await?);
        let observer = PoolOptions::<Turso>::new()
            .max_connections(1)
            .connect_with(TursoConnectOptions::new().filename(&path))
            .await?;
        let request = FleetRequest {
            request_id: RequestId::generate(),
            workspace: WorkspaceId::try_from("00000000-0000-7000-8000-0000000000aa".to_owned())?,
            actor: RecordedActor::Human {
                human_id: HumanId::try_from("cache owner".to_owned())?,
            },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("Visible old state".to_owned())?,
                description: Description::try_from("Cache publication fixture".to_owned())?,
            },
        };
        source.decide(&request).await?;
        let original = source.snapshot().await?;
        assert_eq!(original.through.get(), 2);
        cache.replace_snapshot(&original).await?;
        Ok(Self {
            _directory: directory,
            path,
            source,
            observer,
            cache,
            request,
            original,
        })
    }
    async fn update(&self, name: &str) -> TestResult {
        let mut request = self.request.clone();
        request.request_id = RequestId::generate();
        request.operation = FleetOperation::WorkspaceUpdate {
            name: ResourceName::try_from(name.to_owned())?,
            description: Description::try_from("Cache publication fixture".to_owned())?,
        };
        self.source.decide(&request).await?;
        Ok(())
    }
    async fn ordered_current(&self) -> TestResult<FleetRecordSnapshot> {
        let mut snapshot = self.source.snapshot().await?;
        snapshot.records.sort_by_key(|record| {
            let reference = record.reference();
            (
                reference.id.as_str().to_owned(),
                format!("{:?}", reference.kind),
            )
        });
        Ok(snapshot)
    }
    async fn close(self) -> TestResult {
        Arc::try_unwrap(self.cache)
            .map_err(|_| "cache still has a consumer")?
            .close()
            .await;
        self.observer.close().await;
        self.source.close().await?;
        Ok(())
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn held_catchup_keeps_active_readers_on_the_old_projection_until_one_atomic_publication()
-> TestResult {
    // Inputs come from the actual async Fleet owner. The fixture partitions a
    // genuine feed at the cache seam; Session/FleetClient transport is separate.
    let fixture = Fixture::open().await?;
    fixture.update("Between snapshot pages").await?;
    fixture.update("Newest source state").await?;
    let current = fixture.ordered_current().await?;
    assert_eq!(current.through.get(), 6);
    let first_feed = fixture
        .source
        .feed_fragment(fixture.original.through, 1)
        .await?;
    let remaining = fixture
        .source
        .feed_fragment(first_feed.covered_through, 100)
        .await?;
    assert_eq!(
        (
            first_feed.covered_through.get(),
            first_feed.high_water.get()
        ),
        (3, 6)
    );
    let (staged, staging_ready) = tokio::sync::oneshot::channel();
    let (release, continue_catchup) = tokio::sync::oneshot::channel();
    let cache = fixture.cache.clone();
    let cut = fixture.original.through;
    let pages = current.records.clone();
    let worker = tokio::spawn(async move {
        let mut rebuild = cache.begin_snapshot_rebuild(cut).await?;
        let (first, remaining_records) = pages.split_first().ok_or("missing snapshot records")?;
        assert!(
            first
                .envelope()
                .position
                .fleet_position()
                .is_some_and(|position| position > cut)
        );
        rebuild
            .append_snapshot_page(cut, std::slice::from_ref(first), Some(first.reference()))
            .await?;
        rebuild
            .append_snapshot_page(cut, remaining_records, None)
            .await?;
        // This real older update temporarily replaces the newer snapshot row
        // only in staging. A reader must never observe that intermediate state.
        rebuild
            .apply_catchup_page(
                &first_feed.entries,
                first_feed.covered_through,
                first_feed.high_water,
            )
            .await?;
        staged
            .send(())
            .map_err(|_| "reader stopped before staging")?;
        continue_catchup.await?;
        rebuild
            .apply_catchup_page(
                &remaining.entries,
                remaining.covered_through,
                remaining.high_water,
            )
            .await?;
        rebuild.publish().await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    tokio::time::timeout(Duration::from_secs(5), staging_ready).await??;
    let visible = tokio::time::timeout(Duration::from_secs(5), fixture.cache.snapshot()).await;
    let _ = release.send(());
    let published = tokio::time::timeout(Duration::from_secs(5), worker).await?;
    assert_eq!(visible??, fixture.original);
    published??;
    assert_eq!(
        fixture.cache.snapshot().await?,
        fixture.source.snapshot().await?
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn dropped_rebuild_survives_reopen_as_old_active_rows_and_the_next_build_discards_staging()
-> TestResult {
    let fixture = Fixture::open().await?;
    fixture.update("Abandoned new state").await?;
    let current = fixture.ordered_current().await?;
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(fixture.original.through)
        .await?;
    rebuild
        .append_snapshot_page(fixture.original.through, &current.records, None)
        .await?;
    drop(rebuild);
    assert_eq!(fixture.cache.snapshot().await?, fixture.original);
    assert!(
        sqlx_core::query_scalar::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM fleet_cache_rebuild_records"
        )
        .fetch_one(&fixture.observer)
        .await?
            > 0
    );
    Arc::try_unwrap(fixture.cache)
        .map_err(|_| "cache retained after drop")?
        .close()
        .await;
    let reopened = FleetCache::open(&fixture.path).await?;
    assert_eq!(reopened.snapshot().await?, fixture.original);
    let rebuild = reopened.begin_snapshot_rebuild(current.through).await?;
    assert_eq!(
        sqlx_core::query_scalar::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM fleet_cache_rebuild_records"
        )
        .fetch_one(&fixture.observer)
        .await?,
        0
    );
    drop(rebuild);
    assert_eq!(reopened.snapshot().await?, fixture.original);
    reopened.close().await;
    fixture.observer.close().await;
    fixture.source.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn changed_snapshot_cut_and_gapped_catchup_abort_without_publishing() -> TestResult {
    let fixture = Fixture::open().await?;
    let current = fixture.ordered_current().await?;
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(current.through)
        .await?;
    assert!(matches!(
        rebuild
            .append_snapshot_page(current.through.successor()?, &current.records, None)
            .await,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    assert!(matches!(
        rebuild
            .append_snapshot_page(current.through, &current.records, None)
            .await,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    drop(rebuild);
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(current.through)
        .await?;
    rebuild
        .append_snapshot_page(current.through, &current.records, None)
        .await?;
    assert!(matches!(
        rebuild
            .apply_catchup_page(
                &[],
                current.through.successor()?,
                current.through.successor()?
            )
            .await,
        Err(RouterLocalStorageError::PositionGap)
    ));
    assert!(matches!(
        rebuild.publish().await,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    assert_eq!(fixture.cache.snapshot().await?, fixture.original);
    let early = fixture
        .cache
        .begin_snapshot_rebuild(current.through)
        .await?;
    assert!(matches!(
        early.publish().await,
        Err(RouterLocalStorageError::InvalidCursor)
    ));
    assert_eq!(fixture.cache.snapshot().await?, fixture.original);
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn stored_staging_corruption_is_decoded_before_active_rows_are_replaced() -> TestResult {
    let fixture = Fixture::open().await?;
    let current = fixture.ordered_current().await?;
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(current.through)
        .await?;
    rebuild
        .append_snapshot_page(current.through, &current.records, None)
        .await?;
    rebuild
        .apply_catchup_page(&[], current.through, current.through)
        .await?;
    sqlx_core::query::query("UPDATE fleet_cache_rebuild_records SET payload='{}'")
        .execute(&fixture.observer)
        .await?;
    assert!(matches!(
        rebuild.publish().await,
        Err(RouterLocalStorageError::InvalidRecordAt { .. })
    ));
    assert_eq!(fixture.cache.snapshot().await?, fixture.original);
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_failed_final_insert_rolls_back_the_active_rows_and_checkpoint_together() -> TestResult {
    let fixture = Fixture::open().await?;
    fixture.update("Ready new state").await?;
    let current = fixture.ordered_current().await?;
    let feed = fixture
        .source
        .feed_fragment(fixture.original.through, 100)
        .await?;
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(fixture.original.through)
        .await?;
    rebuild
        .append_snapshot_page(fixture.original.through, &current.records, None)
        .await?;
    rebuild
        .apply_catchup_page(&feed.entries, feed.covered_through, feed.high_water)
        .await?;
    fixture.observer.execute("CREATE TRIGGER refuse_cache_publication BEFORE INSERT ON cached_fleet_records BEGIN SELECT RAISE(ABORT,'controlled publication failure'); END")
        .await?;
    assert!(matches!(
        rebuild.publish().await,
        Err(RouterLocalStorageError::Database(_))
    ));
    assert_eq!(fixture.cache.snapshot().await?, fixture.original);
    sqlx_core::query::query("DROP TRIGGER refuse_cache_publication")
        .execute(&fixture.observer)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_authoritative_full_rebuild_can_replace_a_cache_with_a_different_checkpoint()
-> TestResult {
    let fixture = Fixture::open().await?;
    // A disposable cache does not decide whether the authoritative owner's
    // initial snapshot cut is too old. Cut/progress checks apply within a build.
    let mut rebuild = fixture
        .cache
        .begin_snapshot_rebuild(FleetPosition::ZERO)
        .await?;
    rebuild
        .append_snapshot_page(FleetPosition::ZERO, &[], None)
        .await?;
    rebuild
        .apply_catchup_page(&[], FleetPosition::ZERO, FleetPosition::ZERO)
        .await?;
    rebuild.publish().await?;
    assert_eq!(
        fixture.cache.snapshot().await?,
        FleetRecordSnapshot {
            through: FleetPosition::ZERO,
            records: Vec::new()
        }
    );
    fixture.close().await
}
