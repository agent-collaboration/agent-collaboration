//! Real board writes and native capture; scripted copy answers isolate mapping from networking.
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use board_fleet_sync::{BoardCaptureBookkeeping, BoardPieceAnnotation};
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use durable_steps::{CallFailure, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{CopyRole, InstallId, Piece, ReceiverId, ReplicationAnswer};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use fleet_sync_writer::{
    ReplicationSender, ReplicationTransport, SendFailure, SenderError, SenderOptions,
};
use message_board::{EpochNumber, FeedCursor, ProjectId, TaskId, WorkstreamId};
use runtime_primitives::SystemClock;
use serde_json::{Value, json};
use sqlx::Executor;
use std::{num::NonZeroUsize, sync::Arc};
use tokio::sync::Mutex;
#[path = "capture_feed_marks/cancel_expansions.rs"]
mod cancel_expansions;
#[path = "support/mail_cancel_expansions.rs"]
mod mail_cancel_expansions;

#[derive(Default)]
struct ScriptedCopy {
    pieces: Mutex<Vec<Piece<BoardAnnotation>>>,
    lose_final: Mutex<bool>,
}
impl ReplicationTransport<BoardConsumerProtocol> for ScriptedCopy {
    async fn send_piece(
        &self,
        _: ReceiverId,
        piece: Piece<BoardAnnotation>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        let answer = if let Some(part) = &piece.part
            && part.index + 1 < part.count
        {
            ReplicationAnswer::PartStaged {
                transaction: part.transaction,
                index: part.index,
                acked: piece.continues_from,
            }
        } else if std::mem::take(&mut *self.lose_final.lock().await) {
            self.pieces.lock().await.push(piece);
            return Err(SendFailure::AnswerLost);
        } else {
            ReplicationAnswer::Applied {
                through: piece.through,
            }
        };
        self.pieces.lock().await.push(piece);
        Ok(answer)
    }
}

struct NoCalls;
impl CallRouter for NoCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
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

async fn create_task(
    fixture: &BoardFixture,
    project: &ProjectId,
    workstream: &WorkstreamId,
    _epoch: EpochNumber,
    title: &str,
) -> Result<FeedCursor, TestError> {
    let request = fixture.request(project, json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":title,"body":"","blockedBy":[],"relatedTo":[]}));
    accepted(&fixture.api.call("task_create", request).await?);
    head(fixture, project).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn acknowledgement_mapping_remains_readable_under_the_project_writer_guard()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (_, epoch) = accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database = target._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let start = snapshot.position();
    snapshot.close().await?;
    let admitted = create_task(&target, &project, &workstream, epoch, "ACK guard probe").await?;
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        Arc::new(ScriptedCopy::default()),
        SenderOptions::default(),
    );
    let copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    sender
        .add_copy(copy, CopyRole::Authoritative, start)
        .await?;
    sender.replicate_once(copy).await?;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;

    // Read the real sender state and board mapping while the native guard is
    // held. The timeout only detects a lock cycle; it is not readiness proof.
    let write = store.begin_admission(target.record_origin()?).await?;
    let observed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let acked = sender
            .copies()
            .await?
            .into_iter()
            .find(|state| state.copy == copy)
            .ok_or("copy")?
            .acked;
        Ok::<_, TestError>(store.acknowledged_capture(acked).await?)
    })
    .await;
    write.rollback().await?;
    assert_eq!(observed??.feed, Some(admitted));
    assert_eq!(head(&target, &project).await?, admitted);
    drop(sender);
    drop(store);
    drop(database);
    source.stop().await?;
    target.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn split_capture_maps_only_acknowledged_commits_and_survives_lost_answers()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let target = BoardFixture::start().await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database = target._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let start = snapshot.position();
    snapshot.close().await?;
    let first_head =
        create_task(&target, &project, &workstream, epoch, "first captured task").await?;
    let second_head = create_task(&target, &project, &workstream, epoch, "later live head").await?;
    assert!(second_head.position > first_head.position);
    let transport = Arc::new(ScriptedCopy::default());
    *transport.lose_final.lock().await = true;
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        transport.clone(),
        SenderOptions {
            max_part_items: NonZeroUsize::MIN,
            ..SenderOptions::default()
        },
    );
    let first_copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    let second_copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    sender
        .add_copy(first_copy, CopyRole::Authoritative, start)
        .await?;
    sender
        .add_copy(second_copy, CopyRole::Follower, start)
        .await?;
    assert!(matches!(
        sender.replicate_once(first_copy).await,
        Err(SenderError::Send(SendFailure::AnswerLost))
    ));
    let through = transport.pieces.lock().await.last().ok_or("piece")?.through;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    assert_eq!(
        store.acknowledged_capture(through).await?.feed,
        Some(first_head)
    );
    assert_eq!(
        store.acknowledged_capture(start).await?.feed,
        Some(inherited)
    );
    assert!(
        sender
            .copies()
            .await?
            .iter()
            .any(|copy| copy.copy == first_copy && copy.acked == start)
    );
    {
        let pieces = transport.pieces.lock().await;
        assert!(
            pieces.len() > 1,
            "the test must split a real captured transaction"
        );
        assert!(
            pieces
                .iter()
                .take(pieces.len() - 1)
                .all(|piece| piece.annotation.is_none())
        );
        assert!(matches!(
            pieces.last().and_then(|piece| piece.annotation.as_ref()),
            Some(BoardAnnotation::ProjectSummary { .. })
        ));
    }
    // Recreate the sender with its durable assignments and copy positions intact.
    drop(sender);
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        transport.clone(),
        SenderOptions {
            max_part_items: NonZeroUsize::MIN,
            ..SenderOptions::default()
        },
    );
    sender.replicate_once(second_copy).await?;
    sender.replicate_once(first_copy).await?;
    // Both copies assign the same transaction once. A lost answer is never an acknowledgement.
    assert!(
        sender
            .copies()
            .await?
            .iter()
            .filter(|copy| copy.copy == first_copy || copy.copy == second_copy)
            .all(|copy| copy.acked == through)
    );
    sender.replicate_once(first_copy).await?;
    let final_through = sender
        .copies()
        .await?
        .into_iter()
        .find(|copy| copy.copy == first_copy)
        .ok_or("copy")?
        .acked;
    assert!(final_through > through);
    assert_eq!(
        store.acknowledged_capture(through).await?.feed,
        Some(first_head)
    );
    assert_eq!(
        store.acknowledged_capture(final_through).await?.feed,
        Some(second_head)
    );
    let mut write = database.begin_write().await?;
    sqlx::query("INSERT INTO mail_handoffs VALUES(?,1,'{}')")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut write)
        .await?;
    sqlx::query("INSERT INTO handoff_frontier VALUES(?,1)")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut write)
        .await?;
    write.commit().await?;
    transport.pieces.lock().await.clear();
    sender.replicate_once(first_copy).await?;
    let handoff_through = sender
        .copies()
        .await?
        .into_iter()
        .find(|copy| copy.copy == first_copy)
        .ok_or("copy")?
        .acked;
    let handoff = store.acknowledged_capture(handoff_through).await?;
    assert_eq!(handoff.feed, Some(second_head));
    assert_eq!(handoff.max_handoff_seq, Some(1));
    assert_eq!(handoff.settled_through, Some(1));
    assert!(
        transport
            .pieces
            .lock()
            .await
            .iter()
            .all(|piece| piece.annotation.is_none())
    );
    // Malformed captured metadata refuses before a piece can leave the sender.
    let mut write = database.begin_write().await?;
    (&mut write).execute("UPDATE project_feed SET time_basis='invalid' WHERE position=(SELECT MAX(position) FROM project_feed)").await?;
    write.commit().await?;
    let before = transport.pieces.lock().await.len();
    assert!(matches!(
        sender.replicate_once(first_copy).await,
        Err(SenderError::Bookkeeping(_))
    ));
    assert_eq!(transport.pieces.lock().await.len(), before);
    drop(store);
    drop(sender);
    drop(database);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn published_snapshot_baseline_uses_its_receipt_and_is_unchanged_after_later_writes()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let source_database = source._registry.database(&project).await?;
    let mut write = source_database.begin_write().await?;
    // The fixture exercises capture's counter columns; A6 owns the item body semantics.
    sqlx::query("INSERT INTO mail_handoffs VALUES(?,1,'{}')")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut write)
        .await?;
    sqlx::query("INSERT INTO handoff_frontier VALUES(?,1)")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut write)
        .await?;
    write.commit().await?;
    drop(source_database);
    let target = BoardFixture::start().await?;
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_unprepared_project(&source, &target, &project).await?;
    // The publisher helper closed its native writer without recording a baseline.
    // This fresh registry open exercises the restart ordering seam, before any board write.
    let database = target._registry.database(&project).await?;
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("installed snapshot")?;
    let host = Arc::new(TokioHost::new(Arc::new(NoCalls), SystemClock));
    let journal = database.journal_store(host).await?;
    let position = receipt.install.position;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let early=target.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"before baseline","body":"","blockedBy":[],"relatedTo":[]}));
    let request_id: message_board::RequestId =
        serde_json::from_value(early.get("requestId").ok_or("request ID")?.clone())?;
    let (prepare, waiting) = tokio::sync::oneshot::channel();
    let baseline = receipt.clone();
    let initializing = async {
        waiting.await.map_err(|_| StoreError::Aborted {
            reason: "baseline fixture stopped".to_owned(),
        })?;
        journal
            .rule_transaction(move |scope| {
                Box::pin(async move {
                    message_board_storage::record_snapshot_capture(scope, &baseline)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await
    };
    let concurrent_request = async {
        let reply = target.api.call("task_create", early.clone()).await?;
        assert_eq!(
            reply.pointer("/error/data/kind"),
            Some(&json!("boardUnavailable")),
            "{reply}"
        );
        let mut read = store.snapshot().await?;
        assert_eq!(read.high_water(), inherited);
        assert!(read.record(&request_id.clone().into()).await?.is_none());
        read.finish().await?;
        assert!(matches!(
            store.begin_admission(target.record_origin()?).await,
            Err(message_board_storage::ProjectStorageError::SnapshotBaselinePending)
        ));
        prepare
            .send(())
            .map_err(|_| "baseline initialization stopped")?;
        Ok::<_, TestError>(())
    };
    let (request_result, baseline_result) = tokio::join!(concurrent_request, initializing);
    request_result?;
    baseline_result?;
    let initial = store.acknowledged_capture(position).await?;
    assert_eq!(initial.feed, Some(inherited));
    assert_eq!(initial.max_handoff_seq, Some(1));
    assert_eq!(initial.settled_through, Some(1));
    accepted(&target.api.call("task_create", early).await?);
    let later = create_task(&target, &project, &workstream, epoch, "after baseline").await?;
    assert!(later.position > inherited.position);
    let baseline = receipt.clone();
    journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                message_board_storage::record_snapshot_capture(scope, &baseline)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(store.acknowledged_capture(position).await?, initial);
    let mut wrong = receipt.clone();
    wrong.install.install_id = InstallId::new(uuid::Uuid::now_v7())?;
    let result: Result<(), StoreError> = journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                let result = message_board_storage::record_snapshot_capture(scope, &wrong).await;
                assert!(matches!(
                    result,
                    Err(message_board_storage::ProjectStorageError::InvalidFeed)
                ));
                Err(StoreError::Aborted {
                    reason: "expected mismatched snapshot refusal".to_owned(),
                })
            })
        })
        .await;
    assert!(result.is_err());
    // A matching file receipt also refuses a mismatched stored baseline identity.
    let mut wrong = receipt.clone();
    wrong.install.install_id = InstallId::new(uuid::Uuid::now_v7())?;
    let wrong_json = serde_json::to_string(&wrong.install)?;
    journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                sqlx::query("UPDATE capture_feed_marks SET snapshot_identity=?")
                    .bind(wrong_json)
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                Ok::<_, StoreError>(())
            })
        })
        .await?;
    let baseline = receipt.clone();
    let result: Result<(), StoreError> = journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                let result = message_board_storage::record_snapshot_capture(scope, &baseline).await;
                assert!(matches!(
                    result,
                    Err(message_board_storage::ProjectStorageError::InvalidFeed)
                ));
                Err(StoreError::Aborted {
                    reason: "expected stored snapshot mismatch refusal".to_owned(),
                })
            })
        })
        .await;
    assert!(result.is_err());
    drop(store);
    drop(journal);
    drop(database);
    target.stop().await?;
    source.stop().await
}
