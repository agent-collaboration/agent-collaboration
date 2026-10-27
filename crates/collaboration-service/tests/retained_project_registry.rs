//! Forced real-pool interleavings at the writable-to-superseded registry boundary.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;

use board_fixture::{BoardFixture, TestError};
use collaboration_runtime::ProjectReadHandle;
use collaboration_runtime::ProjectRegistryError;
use collaboration_runtime::ProjectStoreRegistry;
use fleet_sync_protocol::{CommitSequence, Epoch, LeaseRefusal, RenewAnswer, ReplicationPosition};
use message_board::*;
use runtime_primitives::{Clock, SystemClock};
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
    time::Duration,
};
use tokio::time::timeout;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;

const BOUND: Duration = Duration::from_secs(30);

struct NativeDrainStarted(Arc<AtomicBool>);
impl<Subscriber: tracing::Subscriber> tracing_subscriber::Layer<Subscriber> for NativeDrainStarted {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, Subscriber>,
    ) {
        if event.metadata().target() == "collaboration_service::project_native_close" {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

async fn wait_for_native_drain(
    mut transition: Pin<&mut impl Future<Output = Result<(), ProjectRegistryError>>>,
    started: &AtomicBool,
) -> Result<(), TestError> {
    poll_fn(|context| {
        if let Poll::Ready(result) = transition.as_mut().poll(context) {
            return Poll::Ready(Err(format!(
                "native drain completed while its read is held: {result:?}"
            )
            .into()));
        }
        if started.load(Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await
}

async fn wait_for_transition_gate(
    mut transition: Pin<&mut impl Future<Output = Result<(), ProjectRegistryError>>>,
    reader: &ProjectReadHandle,
) -> Result<(), TestError> {
    poll_fn(|context| {
        if let Poll::Ready(result) = transition.as_mut().poll(context) {
            return Poll::Ready(Err(format!(
                "transition finished before the held read was released: {result:?}"
            )
            .into()));
        }
        // Unlike a record query, this probe has no native IO: its only pending
        // step in this fixture is the real registry gate. A first transition
        // poll can still be checking paths, so that poll alone is not readiness.
        let mut probe = std::pin::pin!(tokio::task::unconstrained(reader.schema_state()));
        match probe.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(Ok(())),
            Poll::Ready(Ok(_)) => Poll::Pending,
            Poll::Ready(Err(ProjectRegistryError::Unavailable { .. })) => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
        }
    })
    .await
}

async fn supersede(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
) -> Result<EpochNumber, TestError> {
    // This is the library's persisted authority boundary, supplied as a typed
    // component input. The real FleetService takeover is a separate Host proof.
    let current = Epoch::new(2)?;
    let ended_at = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0));
    registry
        .database(project)
        .await?
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
                current_epoch: current,
                epoch_ended_at: ended_at,
            }),
            SystemClock.now(),
        )
        .await?;
    Ok(EpochNumber::try_from(current.value())?)
}

#[tokio::test]
async fn snapshots_drain_before_swap_and_a_stale_read_handle_becomes_unavailable()
-> Result<(), TestError> {
    timeout(BOUND, async {
        // Arrange a real read already pinned when authority is lost.
        let fixture = BoardFixture::start().await?;
        let (project, _) = fixture.project().await?;
        let reader = fixture._registry.read_project(&project).await?.ok_or("reader")?;
        let stale_writer = fixture._registry.open_project(&project).await?;
        let mut before = reader.snapshot().await?;
        let authority = supersede(&fixture._registry, &project).await?;
        assert!(matches!(reader.snapshot().await, Err(ProjectRegistryError::Unavailable { .. })));

        // Act: the exclusive swap queues behind the old snapshot. Tokio's
        // write-preferring gate also queues the next read from this old handle.
        let mut swapping = Box::pin(fixture._registry.supersede_project(&project));
        wait_for_transition_gate(swapping.as_mut(), &reader).await?;
        let mut next = Box::pin(reader.snapshot());
        assert!(matches!(futures_util::poll!(&mut next), Poll::Pending));
        assert!(matches!(before.record(&project.clone().into()).await?, Some(Record::Project(state)) if state.project_id == project));
        before.finish().await?;
        swapping.await?;
        assert!(matches!(next.await, Err(ProjectRegistryError::Unavailable { .. })));

        // The terminal owner keeps cached authority without a board read capability,
        // including after restart without enrollment.
        assert_eq!(fixture._registry.superseded_current(&project).await, Some(Some(authority)));
        assert!(stale_writer.begin_admission(fixture.record_origin()?).await.is_err());
        assert!(fixture._registry.open_project(&project).await.is_err());
        assert!(fixture._registry.database(&project).await.is_err());
        let bytes = std::fs::read(stale_writer.path())?;
        let restarted = ProjectStoreRegistry::new(fixture._directory.path(), None, Arc::new(SystemClock));
        assert!(matches!(restarted.read_project(&project).await, Err(ProjectRegistryError::Unavailable { .. })));
        assert_eq!(restarted.superseded_current(&project).await, Some(Some(authority)));
        assert_eq!(std::fs::read(stale_writer.path())?, bytes);
        let view = fixture.api.call("view_work", serde_json::json!({"projectId":project})).await?;
        assert_eq!(view.pointer("/error/data/kind"), Some(&serde_json::json!("boardUnavailable")), "{view}");
        restarted.close().await?;
        drop(reader);
        drop(stale_writer);
        fixture.stop().await
    }).await?
}

#[tokio::test]
async fn cancelled_native_close_stays_unavailable_until_the_same_drain_is_retried()
-> Result<(), TestError> {
    timeout(BOUND, async {
        let fixture = BoardFixture::start().await?;
        let (project, _) = fixture.project().await?;
        let database = fixture._registry.database(&project).await?;
        let reader = fixture
            ._registry
            .read_project(&project)
            .await?
            .ok_or("reader")?;
        supersede(&fixture._registry, &project).await?;
        // This real checked-out connection pins NativeFileOwner::close after
        // the registry's exclusive gate is taken; it is not a pool stand-in.
        let held = database.reader().begin_read().await?;
        // The gate precedes an asynchronous authority read. Observe the actual
        // native-drain boundary before cancellation, while the real read pins it.
        let started = Arc::new(AtomicBool::new(false));
        let subscriber = tracing_subscriber::registry().with(NativeDrainStarted(started.clone()));
        let mut closing = Box::pin(
            fixture
                ._registry
                .supersede_project(&project)
                .with_subscriber(subscriber),
        );
        wait_for_native_drain(closing.as_mut(), &started).await?;
        let mut reading = Box::pin(reader.snapshot());
        assert!(matches!(futures_util::poll!(&mut reading), Poll::Pending));
        drop(reading);
        drop(closing);
        assert!(matches!(
            reader.snapshot().await,
            Err(ProjectRegistryError::Unavailable { .. })
        ));
        assert!(fixture._registry.open_project(&project).await.is_err());
        let mut retrying = Box::pin(fixture._registry.supersede_project(&project));
        wait_for_transition_gate(retrying.as_mut(), &reader).await?;
        held.close().await?;
        retrying.await?;
        assert!(matches!(
            reader.snapshot().await,
            Err(ProjectRegistryError::Unavailable { .. })
        ));
        assert!(fixture._registry.database(&project).await.is_err());
        drop(reader);
        drop(database);
        fixture.stop().await
    })
    .await?
}

#[tokio::test]
async fn completed_supersession_retries_without_reopening_the_closed_file() -> Result<(), TestError>
{
    timeout(BOUND, async {
        let fixture = BoardFixture::start().await?;
        let (project, _) = fixture.project().await?;
        let database = fixture._registry.database(&project).await?;
        let reader = fixture
            ._registry
            .read_project(&project)
            .await?
            .ok_or("reader")?;
        supersede(&fixture._registry, &project).await?;
        // Let the registry read the actual successor and close its own database.
        fixture._registry.supersede_project(&project).await?;
        let path = database.path();
        let temporarily_unavailable = path.with_extension("unavailable.sqlite");
        let bytes = std::fs::read(path)?;
        std::fs::rename(path, &temporarily_unavailable)?;

        fixture._registry.supersede_project(&project).await?;
        assert!(!path.exists(), "terminal transition reopened the old file");
        assert_eq!(std::fs::read(&temporarily_unavailable)?, bytes);
        assert!(matches!(
            reader.snapshot().await,
            Err(ProjectRegistryError::Unavailable { .. })
        ));
        assert!(fixture._registry.open_project(&project).await.is_err());
        assert!(fixture._registry.database(&project).await.is_err());

        std::fs::rename(&temporarily_unavailable, path)?;
        fixture._registry.supersede_project(&project).await?;
        assert!(matches!(
            reader.snapshot().await,
            Err(ProjectRegistryError::Unavailable { .. })
        ));
        assert_eq!(std::fs::read(path)?, bytes);
        drop(reader);
        drop(database);
        fixture.stop().await
    })
    .await?
}
