//! The owning native mapping and public view keep failures and accepted-line epochs.
#![allow(clippy::panic_in_result_fn)]
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_service::{
    NativeProjectStore, ProjectReplicationPosition, ReplicatedPositionFuture,
};
use fleet_sync_protocol::ReplicationPosition;
use message_board::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
enum Source {
    Fixed(FeedCursor),
    Native {
        store: Arc<NativeProjectStore>,
        through: ReplicationPosition,
    },
}
#[derive(Default)]
struct ProgressPort {
    source: Mutex<Option<Source>>,
    pause: AtomicBool,
    entered: Notify,
    release: Notify,
}
impl ProjectReplicationPosition for ProgressPort {
    fn acknowledged_mail_changes<'port>(
        &'port self,
        _: &'port ProjectId,
    ) -> collaboration_service::AcknowledgedMailChangesFuture<'port> {
        Box::pin(async move {
            match self.source.lock().await.clone() {
                Some(Source::Native { store, through }) => store
                    .acknowledged_capture(through)
                    .await
                    .map_err(|_| BoardError::board_unavailable()),
                // The view's synthetic feed cursor authorizes no mail handoff.
                Some(Source::Fixed(_)) | None => {
                    Ok(message_board_storage::AcknowledgedProjectChanges::default())
                }
            }
        })
    }

    fn replicated_through<'port>(
        &'port self,
        _: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port> {
        Box::pin(async move {
            if self.pause.load(Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            match self.source.lock().await.clone() {
                Some(Source::Fixed(cursor)) => Ok(cursor),
                Some(Source::Native { store, through }) => {
                    collaboration_service::project_replication_cursor(&store, through, epoch).await
                }
                None => Ok(FeedCursor {
                    epoch,
                    position: FeedPosition::ZERO,
                }),
            }
        })
    }
}

async fn view(fixture: &BoardFixture, project: &ProjectId) -> Result<Value, TestError> {
    Ok(fixture
        .api
        .call("view_work", json!({"projectId":project}))
        .await?)
}
async fn head(fixture: &BoardFixture, project: &ProjectId) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let cursor = read.high_water();
    read.finish().await?;
    Ok(cursor)
}

#[tokio::test]
async fn native_mapping_failure_is_board_unavailable_instead_of_zero_progress()
-> Result<(), TestError> {
    let port = Arc::new(ProgressPort::default());
    let fixture = BoardFixture::start_with_project_replication(port.clone()).await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let database = fixture._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let through = snapshot.position();
    snapshot.close().await?;
    let cursor = head(&fixture, &project).await?;
    let mut write = database.begin_write().await?;
    sqlx::query(
        "INSERT INTO capture_feed_marks(epoch,captured_position,last_feed_position) VALUES(?,?,?)",
    )
    .bind(i64::try_from(through.epoch.value())?)
    .bind(i64::try_from(through.sequence.value())?)
    .bind(i64::try_from(cursor.position.get())?)
    .execute(&mut write)
    .await?;
    write.commit().await?;
    *port.source.lock().await = Some(Source::Native {
        store: store.clone(),
        through,
    });
    let healthy = view(&fixture, &project).await?;
    assert_eq!(
        healthy.pointer("/result/replicatedThrough"),
        Some(&json!(cursor)),
        "{healthy}"
    );
    let mut write = database.begin_write().await?;
    sqlx::query("UPDATE capture_feed_marks SET last_feed_position=-1")
        .execute(&mut write)
        .await?;
    write.commit().await?;
    assert!(store.acknowledged_capture(through).await.is_err());
    let failed = view(&fixture, &project).await?;
    assert_eq!(
        failed.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{failed}"
    );
    *port.source.lock().await = None;
    drop(store);
    drop(database);
    fixture.stop().await
}

#[tokio::test]
async fn acknowledged_inherited_prefix_keeps_the_epoch_that_assigned_its_rows()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let port = Arc::new(ProgressPort::default());
    let target = BoardFixture::start_with_project_replication(port.clone()).await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database = target._registry.database(&project).await?;
    let through = database
        .installed_snapshot()
        .await?
        .ok_or("installed snapshot")?
        .install
        .position;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    *port.source.lock().await = Some(Source::Native {
        store: store.clone(),
        through,
    });
    let before_write = view(&target, &project).await?;
    assert_eq!(
        before_write.pointer("/result/asOf"),
        Some(&json!(inherited)),
        "{before_write}"
    );
    assert_eq!(
        before_write.pointer("/result/replicatedThrough"),
        Some(&json!(inherited)),
        "{before_write}"
    );
    assert_eq!(
        before_write.pointer("/result/currentWriter"),
        Some(&json!({"kind":"held","epoch":epoch})),
        "{before_write}"
    );
    let request=target.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"current epoch","body":"","blockedBy":[],"relatedTo":[]}));
    accepted(&target.api.call("task_create", request).await?);
    let reply = view(&target, &project).await?;
    assert_eq!(
        reply.pointer("/result/replicatedThrough"),
        Some(&json!(inherited)),
        "{reply}"
    );
    assert_eq!(
        reply.pointer("/result/asOf/epoch"),
        Some(&json!(epoch)),
        "{reply}"
    );
    *port.source.lock().await = None;
    drop(store);
    drop(database);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
async fn acknowledgement_after_the_pinned_view_is_bounded_to_that_views_as_of()
-> Result<(), TestError> {
    let port = Arc::new(ProgressPort::default());
    let fixture = BoardFixture::start_with_project_replication(port.clone()).await?;
    let (project, workstream) = fixture.project().await?;
    let before = head(&fixture, &project).await?;
    port.pause.store(true, Ordering::SeqCst);
    let viewing = view(&fixture, &project);
    let advancing = async {
        port.entered.notified().await;
        fixture.task(&project, &workstream).await?;
        let after = head(&fixture, &project).await?;
        assert!(after.position > before.position);
        *port.source.lock().await = Some(Source::Fixed(after));
        port.release.notify_one();
        Ok::<_, TestError>(())
    };
    let (reply, advanced) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        tokio::join!(viewing, advancing)
    })
    .await?;
    advanced?;
    let reply = reply?;
    assert_eq!(
        reply.pointer("/result/asOf"),
        Some(&json!(before)),
        "{reply}"
    );
    assert_eq!(
        reply.pointer("/result/replicatedThrough"),
        Some(&json!(before)),
        "{reply}"
    );
    port.pause.store(false, Ordering::SeqCst);
    fixture.stop().await
}
