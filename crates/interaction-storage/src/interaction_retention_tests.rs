//! Fixed-clock retention keeps active work and prunes completed rows atomically.
use super::*;
use crate::MailPushUpdate;
use collaboration_mail_protocol::{CancelCause, MailItemId};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MailReadContext, MessageSendResult, QueuedMessageSend,
};
use fleet_sync_protocol::{DeliveryFailureKind, DeliveryRefusal, RecipientOutcome};
use message_board::{EpochNumber, ProjectId};

fn at(value: &str) -> chrono::DateTime<chrono::Utc> {
    value.parse().expect("valid fixed timestamp")
}

fn as_mail_item(push_id: &collaboration_protocol::PushId) -> MailItemId {
    push_id
        .as_str()
        .to_owned()
        .try_into()
        .expect("mail item UUID")
}

fn queued_answer(
    item: MailItemId,
    target: collaboration_protocol::SessionRef,
) -> MessageSendResult {
    MessageSendResult::Queued {
        queued: QueuedMessageSend {
            item,
            target,
            target_identity: "retention recipient".to_owned(),
        },
    }
}

async fn record_answer(
    store: &NativeStore,
    item_id: MailItemId,
    target: collaboration_protocol::SessionRef,
    now: chrono::DateTime<chrono::Utc>,
) {
    let answer = queued_answer(item_id.clone(), target);
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::record_message_send_admission(scope, &item_id, "retention recipient", now)
                    .await
                    .map_err(StoreError::backend)?;
                crate::record_message_send_outcome(scope, &item_id, &answer, now.timestamp_millis())
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .expect("test answer should persist");
}

#[tokio::test]
async fn two_calendar_month_cutoff_keeps_boundary_and_all_nonterminal_pushes() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("Turso test store");
    let now = at("2026-10-31T12:00:00.000000000Z");
    let cutoff = at("2026-08-31T12:00:00.000000000Z");
    let cases = [
        (cutoff - chrono::Duration::nanoseconds(1), "delivered"),
        (cutoff, "delivered"),
        (cutoff + chrono::Duration::nanoseconds(1), "delivered"),
        (cutoff - chrono::Duration::days(1), "pending"),
        (cutoff - chrono::Duration::days(1), "held"),
        (cutoff - chrono::Duration::days(1), "attempted"),
    ];
    let mut push_ids = Vec::new();
    for (created_at, state) in cases {
        let (_, mut draft) = native_item_and_push();
        draft.created_at = created_at;
        let push_id = draft.push_id.clone();
        owner.insert_push_record(draft).await.expect("fixture push");
        sqlx::query::<sqlx_turso::Turso>(
            "UPDATE router_pushes SET delivery_state=? WHERE push_id=?",
        )
        .bind(state)
        .bind(push_id.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .expect("set fixture state");
        push_ids.push((push_id, state));
    }

    assert_eq!(owner.prune_push_records(now, 500).await.unwrap(), 1);
    let mut connection = owner.writer.lock().await;
    let retained =
        sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT count(*) FROM router_pushes")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(retained, 5);
    for (push_id, _) in &push_ids[1..] {
        let exists = sqlx::query_scalar::<sqlx_turso::Turso, i64>(
            "SELECT count(*) FROM router_pushes WHERE push_id=?",
        )
        .bind(push_id.as_str())
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(exists, 1, "boundary or active push was pruned");
    }
    drop(connection);
    owner.close().await.unwrap();
}

#[tokio::test]
async fn prunes_approval_routes_by_last_use_and_keeps_the_cutoff_boundary() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("Turso test store");
    let now = at("2026-10-31T12:00:00.000000000Z");
    let cutoff_ms = at("2026-08-31T12:00:00.000000000Z").timestamp_millis();
    for (thread_id, last_used_at_ms) in [
        ("expired-route-one", cutoff_ms - 1),
        ("expired-route-two", cutoff_ms - 2),
        ("boundary-route", cutoff_ms),
        ("recently-used-route", cutoff_ms - 1),
    ] {
        sqlx::query::<sqlx_turso::Turso>(
            "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?, '{}', ?)",
        )
        .bind(thread_id)
        .bind(last_used_at_ms)
        .execute(&mut *owner.writer.lock().await)
        .await
        .expect("route fixture row");
    }
    assert_eq!(
        owner
            .approval_route_json("recently-used-route", now.timestamp_millis())
            .await
            .expect("read and touch recent route"),
        Some("{}".to_owned())
    );

    let first_batch = owner
        .prune_push_records(now, 1)
        .await
        .expect("first prune batch");
    let second_batch = owner
        .prune_push_records(now, 1)
        .await
        .expect("second prune batch");
    let empty_batch = owner
        .prune_push_records(now, 1)
        .await
        .expect("empty prune batch");
    let retained =
        sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT count(*) FROM approval_routes")
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .expect("read retained routes");

    assert_eq!(first_batch, 1);
    assert_eq!(second_batch, 1);
    assert_eq!(empty_batch, 0);
    assert_eq!(retained, 2);
    owner.close().await.expect("close test store");
}

#[tokio::test]
async fn pending_and_held_messages_keep_their_recorded_send_answers() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("Turso test store"),
    );
    let native = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let now = at("2026-10-31T12:00:00.000000000Z");
    let old = at("2026-07-01T12:00:00.000000000Z");
    let mut item_ids = Vec::new();
    for state in ["pending", "held"] {
        let (_, mut draft) = native_item_and_push();
        draft.created_at = old;
        let item_id = as_mail_item(&draft.push_id);
        let push_id = draft.push_id.clone();
        let target = draft.target.clone();
        owner.insert_push_record(draft).await.expect("fixture push");
        if state == "held" {
            sqlx::query::<sqlx_turso::Turso>(
                "UPDATE router_pushes SET delivery_state='held' WHERE push_id=?",
            )
            .bind(push_id.as_str())
            .execute(&mut *owner.writer.lock().await)
            .await
            .expect("hold fixture push");
        }
        record_answer(&native, item_id.clone(), target, old).await;
        item_ids.push(item_id);
    }

    assert_eq!(owner.prune_push_records(now, 500).await.unwrap(), 0);
    for item_id in &item_ids {
        let answer = owner
            .message_send_answer(item_id)
            .await
            .unwrap()
            .expect("active message keeps replay answer");
        assert!(answer.result.is_some());
        assert_eq!(answer.settled_at, Some(old));
    }
    let pushes: i64 =
        sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT count(*) FROM router_pushes")
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .unwrap();
    assert_eq!(pushes, 2);
    drop(native);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("native test adapter retained the store"))
        .close()
        .await
        .unwrap();
}

#[tokio::test]
async fn active_send_cancel_retry_and_poll_work_blocks_parent_deletion() {
    #[derive(Clone, Copy)]
    enum ActiveMailChildWork {
        SendAnswer,
        LocalCancel,
        DeliveryRetry,
        PendingPoll,
    }

    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("Turso test store");
    let now = at("2026-10-31T12:00:00.000000000Z");
    let old = at("2026-07-01T12:00:00.000000000Z");
    let mut push_ids = Vec::new();
    for blocker in [
        ActiveMailChildWork::SendAnswer,
        ActiveMailChildWork::LocalCancel,
        ActiveMailChildWork::DeliveryRetry,
        ActiveMailChildWork::PendingPoll,
    ] {
        let (_, mut draft) = native_item_and_push();
        draft.created_at = old;
        let item_id = as_mail_item(&draft.push_id);
        let push_id = draft.push_id.clone();
        owner.insert_push_record(draft).await.expect("fixture push");
        sqlx::query::<sqlx_turso::Turso>(
            "UPDATE router_pushes SET delivery_state='delivered' WHERE push_id=?",
        )
        .bind(push_id.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
        let mut connection = owner.writer.lock().await;
        match blocker {
            ActiveMailChildWork::SendAnswer => {
                sqlx::query("INSERT INTO message_send_answers(item_id,target_identity,result,recorded_at) VALUES (?, 'pending', NULL, ?)")
                    .bind(item_id.as_str()).bind(old.to_rfc3339())
                    .execute(&mut *connection).await.unwrap();
            }
            ActiveMailChildWork::LocalCancel => {
                let cancel_id = uuid::Uuid::now_v7().to_string();
                sqlx::query("INSERT INTO local_cancel_deliveries(item_id,target_item,attribution_json,outcome_json,recorded_at) VALUES (?,?,'{\"kind\":\"bySender\"}',NULL,?)")
                    .bind(cancel_id).bind(item_id.as_str()).bind(old.to_rfc3339())
                    .execute(&mut *connection).await.unwrap();
            }
            ActiveMailChildWork::DeliveryRetry => {
                sqlx::query("INSERT INTO local_delivery_retries(item_id,retry_attempts,scheduled_retry_at) VALUES (?,1,?)")
                    .bind(item_id.as_str()).bind(now.to_rfc3339())
                    .execute(&mut *connection).await.unwrap();
            }
            ActiveMailChildWork::PendingPoll => {
                sqlx::query("INSERT INTO pending_poll_mail(item_id,recipient_service_id,recipient_endpoint_id,recipient_session_id,membership_id,received_at) VALUES (?,'018f47d2-24d5-7a68-b9ec-6f759c39458f','codex-local','retention-poll','membership',?)")
                    .bind(item_id.as_str()).bind(old.to_rfc3339())
                    .execute(&mut *connection).await.unwrap();
            }
        }
        drop(connection);
        push_ids.push(push_id);
    }

    assert_eq!(owner.prune_push_records(now, 500).await.unwrap(), 0);
    let retained: i64 =
        sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT count(*) FROM router_pushes")
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .unwrap();
    assert_eq!(retained, 4, "a parent with active child work was deleted");
    owner.close().await.unwrap();
}

#[tokio::test]
async fn settled_related_rows_and_parent_prune_atomically_as_one_turso_transaction() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("Turso test store"),
    );
    let native = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let now = at("2026-10-31T12:00:00.000000000Z");
    let cutoff = at("2026-08-31T12:00:00.000000000Z");
    let old = cutoff - chrono::Duration::milliseconds(1);
    let (item, mut draft) = native_item_and_push();
    draft.created_at = old;
    let item_id: MailItemId = item.id().as_uuid().to_string().try_into().unwrap();
    let push_id = draft.push_id.clone();
    let cancel_id: MailItemId = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let target = draft.target.clone();
    let project = ProjectId::generate();
    let context = MailReadContext::Direct {
        origin: collaboration_protocol::PushOrigin::OwnerUnverified,
        recipient: target.clone(),
    };
    let send_answer = queued_answer(item_id.clone(), target.clone());
    let cancel_outcome =
        RecipientOutcome::<collaboration_mail_protocol::CarrierAcceptance>::Refused {
            reason: DeliveryRefusal::ProcessingFailed {
                kind: DeliveryFailureKind::ExecutionFailed,
            },
        };
    let persisted_item = item_id.clone();
    let persisted_cancel = cancel_id.clone();
    let persisted_project = project.clone();
    let persisted_context = context.clone();
    native
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, draft)
                    .await
                    .map_err(StoreError::backend)?;
                crate::mark_mail_push_attempted_in_scope(scope, &push_id)
                    .await
                    .map_err(StoreError::backend)?;
                crate::apply_mail_push_outcome_in_scope(
                    scope,
                    &push_id,
                    MailPushUpdate::Finished {
                        receipt: DeliveryReceipt {
                            outcome: DeliveryOutcome::PeerMessageWritten,
                            reachability: None,
                            client: None,
                        },
                        settled_at: old,
                    },
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_message_send_admission(
                    scope,
                    &persisted_item,
                    "retention recipient",
                    old,
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_message_send_outcome(
                    scope,
                    &persisted_item,
                    &send_answer,
                    old.timestamp_millis(),
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_mail_read_context_in_scope(
                    scope,
                    &persisted_item,
                    &persisted_context,
                    old,
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_mail_submission_epoch_in_scope(
                    scope,
                    &persisted_item,
                    &persisted_project,
                    EpochNumber::try_from(2).unwrap(),
                    old,
                )
                .await
                .map_err(StoreError::backend)?;
                crate::prepare_local_cancel_in_scope(
                    scope,
                    &persisted_cancel,
                    &persisted_item,
                    &CancelCause::BoardDerived {},
                    old,
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_local_cancel_outcome_in_scope(
                    scope,
                    &persisted_cancel,
                    &persisted_item,
                    &cancel_outcome,
                    old.timestamp_millis(),
                )
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .expect("seed completed related projections");

    sqlx::raw_sql("CREATE TRIGGER reject_retained_parent_delete BEFORE DELETE ON router_pushes BEGIN SELECT RAISE(ABORT,'fixture parent delete failure'); END")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(owner.prune_push_records(now, 500).await.is_err());
    for (table, query) in [
        (
            "router_pushes",
            "SELECT count(*) FROM router_pushes WHERE push_id=?",
        ),
        (
            "message_send_answers",
            "SELECT count(*) FROM message_send_answers WHERE item_id=?",
        ),
        (
            "local_cancel_deliveries",
            "SELECT count(*) FROM local_cancel_deliveries WHERE target_item=?",
        ),
        (
            "mail_read_contexts",
            "SELECT count(*) FROM mail_read_contexts WHERE item_id=?",
        ),
        (
            "mail_submission_epochs",
            "SELECT count(*) FROM mail_submission_epochs WHERE item_id=?",
        ),
    ] {
        let count = sqlx::query_scalar::<sqlx_turso::Turso, i64>(query)
            .bind(item_id.as_str())
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .unwrap();
        assert_eq!(count, 1, "transaction partially pruned {table}");
    }
    sqlx::raw_sql("DROP TRIGGER reject_retained_parent_delete")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();

    assert_eq!(owner.prune_push_records(now, 500).await.unwrap(), 5);
    for (table, query) in [
        (
            "router_pushes",
            "SELECT count(*) FROM router_pushes WHERE push_id=?",
        ),
        (
            "message_send_answers",
            "SELECT count(*) FROM message_send_answers WHERE item_id=?",
        ),
        (
            "mail_read_contexts",
            "SELECT count(*) FROM mail_read_contexts WHERE item_id=?",
        ),
        (
            "mail_submission_epochs",
            "SELECT count(*) FROM mail_submission_epochs WHERE item_id=?",
        ),
    ] {
        let count = sqlx::query_scalar::<sqlx_turso::Turso, i64>(query)
            .bind(item_id.as_str())
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .unwrap();
        assert_eq!(count, 0, "eligible {table} row survived");
    }
    let cancels: i64 = sqlx::query_scalar::<sqlx_turso::Turso, i64>(
        "SELECT count(*) FROM local_cancel_deliveries WHERE item_id=?",
    )
    .bind(cancel_id.as_str())
    .fetch_one(&mut *owner.writer.lock().await)
    .await
    .unwrap();
    assert_eq!(cancels, 0);
    drop(native);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("native test adapter retained the store"))
        .close()
        .await
        .unwrap();
}
