//! Child eligibility follows its captured transaction, independently of its acknowledged parent.
use super::mail_cancel_expansions::FrozenFollowCancels;
use super::*;
use message_board::CancelExpansionSeq;

#[tokio::test]
async fn child_capture_marks_wait_for_final_ack_and_survive_all_native_owners_reopening()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let source_store = source
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("source")?;
    let mut write = source_store
        .begin_admission(source.record_origin()?)
        .await?;
    let frozen = FrozenFollowCancels::arrange(&mut write, &project).await?;
    write.commit().await?;
    drop(source_store);
    let target = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database = target._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let start = snapshot.position();
    snapshot.close().await?;
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("target")?;
    let inherited = store.acknowledged_capture(start).await?;
    assert_eq!(inherited.max_handoff_seq, Some(4));
    assert_eq!(inherited.max_expansion_seq, None);
    let first = frozen.child(frozen.targets.first().ok_or("first target")?)?;
    let second = frozen.child(frozen.targets.get(1).ok_or("second target")?)?;
    let expected_first = serde_json::to_value(&first.envelope)?;
    let expected_second = serde_json::to_value(&second.envelope)?;
    let mut write = store.begin_admission(target.record_origin()?).await?;
    assert_eq!(write.append_mail_cancel_expansion(&first).await?.get(), 1);
    assert_eq!(write.append_mail_cancel_expansion(&second).await?.get(), 2);
    write.commit().await?;
    let transport = Arc::new(ScriptedCopy::default());
    *transport.lose_final.lock().await = true;
    let copy = ReceiverId::new(uuid::Uuid::now_v7())?;
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
    sender
        .add_copy(copy, CopyRole::Authoritative, start)
        .await?;
    assert!(matches!(
        sender.replicate_once(copy).await,
        Err(SenderError::Send(SendFailure::AnswerLost))
    ));
    let partial_ack = sender
        .copies()
        .await?
        .into_iter()
        .find(|entry| entry.copy == copy)
        .ok_or("copy")?
        .acked;
    assert_eq!(partial_ack, start);
    {
        let pieces = transport.pieces.lock().await;
        assert!(
            pieces.len() > 1,
            "the actual child transaction was not split"
        );
        assert!(pieces.iter().any(|piece| piece.transactions.iter().any(|transaction| matches!(transaction, fleet_sync_protocol::ReplicatedTransaction::Rows { rows, .. } if rows.iter().any(|row| matches!(row, fleet_sync_protocol::RowChange::Upsert { table, .. } if table.as_str()=="mail_cancel_expansions"))))));
    }
    let staged = store.acknowledged_capture(partial_ack).await?;
    assert_eq!(staged.max_handoff_seq, Some(4));
    assert_eq!(
        staged.max_expansion_seq, None,
        "the acknowledged parent authorized an unacknowledged child"
    );
    drop(sender);
    drop(store);
    drop(database);
    let directory = target.stop_and_keep_directory().await?;
    let registry = board_fixture::new_registry(directory.path())?;
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("reopened target")?;
    assert_eq!(
        store.acknowledged_capture(partial_ack).await?,
        staged,
        "physical reopen changed child eligibility before any replication"
    );
    let database = registry.database(&project).await?;
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        transport,
        SenderOptions {
            max_part_items: NonZeroUsize::MIN,
            ..SenderOptions::default()
        },
    );
    sender.replicate_once(copy).await?;
    let final_ack = sender
        .copies()
        .await?
        .into_iter()
        .find(|entry| entry.copy == copy)
        .ok_or("reopened copy")?
        .acked;
    assert!(final_ack > start);
    assert_eq!(
        store.acknowledged_capture(start).await?,
        staged,
        "later capture leaked into the older acknowledged prefix"
    );
    let completed = store.acknowledged_capture(final_ack).await?;
    assert_eq!(completed.max_expansion_seq, Some(2));
    let mut read = store.snapshot().await?;
    let page = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 10)
        .await?;
    let [restored_first, restored_second] = page.as_slice() else {
        return Err("two frozen children".into());
    };
    assert_eq!(restored_first.expansion.id()?, first.id()?);
    assert_eq!(restored_second.expansion.id()?, second.id()?);
    assert_eq!(
        serde_json::to_value(&restored_first.expansion.envelope)?,
        expected_first
    );
    assert_eq!(
        serde_json::to_value(&restored_second.expansion.envelope)?,
        expected_second
    );
    read.finish().await?;
    drop(sender);
    drop(database);
    drop(store);
    Arc::try_unwrap(registry)
        .map_err(|_| "reopened registry still owned")?
        .close()
        .await?;
    source.stop().await?;
    Ok(())
}

#[tokio::test]
async fn verified_seed_baseline_inherits_child_marks_and_later_expansions_cannot_change_it()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let store = source
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("source")?;
    let mut write = store.begin_admission(source.record_origin()?).await?;
    let frozen = FrozenFollowCancels::arrange(&mut write, &project).await?;
    let first = frozen.child(frozen.targets.first().ok_or("target")?)?;
    write.append_mail_cancel_expansion(&first).await?;
    write.commit().await?;
    let target = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let database = target._registry.database(&project).await?;
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("verified seed")?;
    assert!(receipt.verified);
    let inherited = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("inherited")?;
    let baseline = inherited
        .acknowledged_capture(receipt.install.position)
        .await?;
    assert_eq!(
        baseline.max_expansion_seq,
        Some(1),
        "seed baseline omitted inherited child associations"
    );
    let mut read = inherited.snapshot().await?;
    let saved = read
        .mail_cancel_expansion_for_target(&first.group_handoff, &first.target_item)
        .await?
        .ok_or("inherited child")?;
    assert_eq!(saved.expansion.id()?, first.id()?);
    assert_eq!(
        serde_json::to_value(&saved.expansion.envelope)?,
        serde_json::to_value(&first.envelope)?
    );
    read.finish().await?;
    let second = frozen.child(frozen.targets.get(1).ok_or("next target")?)?;
    let mut write = inherited.begin_admission(target.record_origin()?).await?;
    assert_eq!(write.append_mail_cancel_expansion(&second).await?.get(), 2);
    write.commit().await?;
    assert_eq!(
        inherited
            .acknowledged_capture(receipt.install.position)
            .await?,
        baseline,
        "post-admission child leaked into pinned seed baseline"
    );
    drop(inherited);
    drop(database);
    drop(store);
    target.stop().await?;
    source.stop().await?;
    Ok(())
}

struct DelayedOldAcknowledgement {
    old: Mutex<Option<fleet_sync_protocol::ReplicationPosition>>,
}
impl ReplicationTransport<BoardConsumerProtocol> for DelayedOldAcknowledgement {
    async fn send_piece(
        &self,
        _: ReceiverId,
        piece: Piece<BoardAnnotation>,
    ) -> Result<ReplicationAnswer, SendFailure> {
        Ok(ReplicationAnswer::Applied {
            through: self.old.lock().await.take().unwrap_or(piece.through),
        })
    }
}

#[tokio::test]
async fn discarded_child_ordinal_reuse_does_not_transfer_a_delayed_old_epoch_acknowledgement()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let store = source
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("source")?;
    let mut write = store.begin_admission(source.record_origin()?).await?;
    let frozen = FrozenFollowCancels::arrange(&mut write, &project).await?;
    write.commit().await?;
    drop(store);
    let old_holder = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&source, &old_holder, &project).await?;
    // The successor inherits the accepted prefix before child X is allocated.
    let successor = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&old_holder, &successor, &project).await?;
    let old_database = old_holder._registry.database(&project).await?;
    let snapshot = old_database.snapshot().await?;
    let old_start = snapshot.position();
    snapshot.close().await?;
    let old_store = old_holder
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("old holder")?;
    let target = frozen.targets.first().ok_or("target")?;
    let discarded = frozen.child(target)?;
    let mut write = old_store
        .begin_admission(old_holder.record_origin()?)
        .await?;
    assert_eq!(
        write.append_mail_cancel_expansion(&discarded).await?.get(),
        1
    );
    write.commit().await?;
    let lost = Arc::new(ScriptedCopy::default());
    *lost.lose_final.lock().await = true;
    let old_copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    let old_sender = ReplicationSender::new(
        old_database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        lost.clone(),
        SenderOptions::default(),
    );
    old_sender
        .add_copy(old_copy, CopyRole::Authoritative, old_start)
        .await?;
    assert!(matches!(
        old_sender.replicate_once(old_copy).await,
        Err(SenderError::Send(SendFailure::AnswerLost))
    ));
    let delayed = lost
        .pieces
        .lock()
        .await
        .last()
        .ok_or("old captured piece")?
        .through;
    drop(old_sender);
    drop(old_database);
    drop(old_store);
    let old_directory = old_holder.stop_and_keep_directory().await?;

    let database = successor._registry.database(&project).await?;
    let snapshot = database.snapshot().await?;
    let start = snapshot.position();
    snapshot.close().await?;
    let store = successor
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("successor")?;
    let replacement = frozen.child(target)?;
    assert_ne!(replacement.id()?, discarded.id()?);
    let mut write = store.begin_admission(successor.record_origin()?).await?;
    assert_eq!(
        write
            .append_mail_cancel_expansion(&replacement)
            .await?
            .get(),
        1,
        "the discarded ordinal was not actually reused"
    );
    write.commit().await?;
    let copy = ReceiverId::new(uuid::Uuid::now_v7())?;
    let sender = ReplicationSender::new(
        database.clone(),
        Arc::new(BoardPieceAnnotation),
        Arc::new(BoardCaptureBookkeeping),
        Arc::new(DelayedOldAcknowledgement {
            old: Mutex::new(Some(delayed)),
        }),
        SenderOptions::default(),
    );
    sender
        .add_copy(copy, CopyRole::Authoritative, start)
        .await?;
    assert!(
        matches!(
            sender.replicate_once(copy).await,
            Err(SenderError::Invalid {
                field: "answer.through"
            })
        ),
        "a real sender accepted the delayed old-epoch acknowledgement"
    );
    let before = sender
        .copies()
        .await?
        .into_iter()
        .find(|entry| entry.copy == copy)
        .ok_or("successor copy")?
        .acked;
    assert_eq!(before, start);
    assert_eq!(
        store.acknowledged_capture(before).await?.max_expansion_seq,
        None
    );
    assert_eq!(
        store.acknowledged_capture(delayed).await?.max_expansion_seq,
        None,
        "old file/epoch mark authorized a different UUID at the reused ordinal"
    );
    sender.replicate_once(copy).await?;
    let own_ack = sender
        .copies()
        .await?
        .into_iter()
        .find(|entry| entry.copy == copy)
        .ok_or("successor copy")?
        .acked;
    assert!(own_ack.epoch > delayed.epoch);
    assert_eq!(
        store.acknowledged_capture(own_ack).await?.max_expansion_seq,
        Some(1)
    );
    let mut read = store.snapshot().await?;
    let child = read
        .mail_cancel_expansion_for_target(&replacement.group_handoff, target)
        .await?
        .ok_or("replacement child")?;
    assert_eq!(child.expansion.id()?, replacement.id()?);
    read.finish().await?;
    drop(sender);
    drop(database);
    drop(store);
    successor.stop().await?;
    source.stop().await?;
    drop(old_directory);
    Ok(())
}
