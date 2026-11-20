//! The real readiness SQL uses the target index for ordered, eligible identities.
use super::*;
use crate::{MailPushUpdate, PendingPollMail, StorageError};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, RouterOriginRef};
use sqlx::{Connection, Row};

fn held_receipt() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "reader absent".into(),
        },
        reachability: None,
        client: None,
    }
}

#[tokio::test]
async fn readiness_ids_search_the_target_index_and_match_eligible_record_order() {
    // Native projection rows stand in for custody; this storage case makes no SDK admission claim.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let (_, prototype) = native_item_and_push();
    let target = prototype.target.clone();
    let body = "x".repeat(65_536);
    let mut drafts = Vec::new();
    let mut expected = Vec::new();
    for ordinal in 0..250 {
        let (_, mut draft) = native_item_and_push();
        draft.created_at = prototype.created_at + chrono::Duration::milliseconds(ordinal % 3);
        draft.body = Some(body.clone());
        expected.push((draft.created_at, draft.push_id.clone()));
        drafts.push((draft, true));
    }
    expected.sort_by(|(left_at, left_id), (right_at, right_id)| {
        left_at
            .cmp(right_at)
            .then_with(|| left_id.as_str().cmp(right_id.as_str()))
    });
    let expected = expected.into_iter().map(|(_, id)| id).collect::<Vec<_>>();
    let (_, mut foreign) = native_item_and_push();
    foreign.target.session_id = "another-held-reader".to_owned().try_into().unwrap();
    drafts.push((foreign, true));
    let (_, pending) = native_item_and_push();
    drafts.push((pending, false));
    let (_, mut scheduled) = native_item_and_push();
    scheduled.body = Some(body.clone());
    let scheduled_id = scheduled.push_id.clone();
    let scheduled_for_transaction = scheduled_id.clone();
    drafts.push((scheduled, true));
    let (poll_item, mut polled) = native_item_and_push();
    polled.body = Some(body.clone());
    let poll_marker = PendingPollMail {
        item: poll_item.id(),
        recipient: polled.target.clone(),
        membership: message_board::RecordId::generate(),
        received_at: polled.created_at,
    };
    let polled_id = polled.push_id.clone();
    drafts.push((polled, true));
    let (_, mut notification) = native_item_and_push();
    let project = message_board::ProjectId::generate();
    let intent = message_board::NotifyIntentId::generate();
    notification.kind = PushKind::BoardNotification;
    notification.origin = PushOrigin::Router(PushKind::BoardNotification);
    notification.origin_router_ref = Some(
        RouterOriginRef::BoardIntent {
            project: project.clone(),
            intent: intent.clone(),
        }
        .canonical_string()
        .unwrap(),
    );
    notification.mode = None;
    notification.body = None;
    notification.header_facts = PushHeaderFacts::BoardNotification {
        project,
        intent,
        cause: message_board::RoleAssignmentId::generate().into(),
        summary: "Legacy board notification".to_owned().try_into().unwrap(),
    };
    let notification_id = notification.push_id.clone();
    drafts.push((notification, true));
    let excluded = [scheduled_id, polled_id, notification_id];
    let excluded_for_claim = excluded.clone();
    let copied_id = prototype.push_id.clone();
    owner.insert_push_record(prototype).await.unwrap();
    owner
        .hold_unsent_direct_message(&copied_id, held_receipt())
        .await
        .unwrap();
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                for (draft, held) in drafts {
                    let id = draft.push_id.clone();
                    crate::insert_mail_push_in_scope(scope, draft)
                        .await
                        .map_err(StoreError::backend)?;
                    if held {
                        crate::apply_mail_push_outcome_in_scope(
                            scope,
                            &id,
                            MailPushUpdate::Held {
                                receipt: held_receipt(),
                                held_at: chrono::Utc::now(),
                            },
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    }
                }
                crate::record_local_delivery_retry_in_scope(
                    scope,
                    &scheduled_for_transaction,
                    true,
                    Some(chrono::Utc::now() + chrono::Duration::hours(1)),
                )
                .await
                .map_err(StoreError::backend)?;
                // Pair a valid marker with the same ID as a held DM projection to
                // exercise the anti-join independently of the notification kind filter.
                crate::insert_pending_poll_in_scope(scope, &poll_marker)
                    .await
                    .map_err(StoreError::backend)?;
                for id in &excluded_for_claim {
                    assert!(
                        !crate::mark_released_mail_pending_in_scope(scope, id)
                            .await
                            .map_err(StoreError::backend)?,
                        "readiness claim accepted excluded push {id:?}"
                    );
                }
                Ok(())
            })
        })
        .await
        .unwrap();
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("readiness owner retained"))
        .close()
        .await
        .unwrap();

    let owner = InteractionStore::open(&path).await.unwrap();
    let started = std::time::Instant::now();
    let ids = owner.held_mail_push_ids(&target).await.unwrap();
    let identity_read = started.elapsed();
    let started = std::time::Instant::now();
    let records = owner.list_held_mail_push_records(&target).await.unwrap();
    let record_read = started.elapsed();
    assert_eq!(ids, expected);
    assert_eq!(
        records
            .iter()
            .filter(|record| !excluded.contains(&record.push_id))
            .map(|record| record.push_id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        records
            .iter()
            .filter(|record| record.kind == PushKind::DirectMessage)
            .all(|record| record.body.as_deref() == Some(body.as_str()))
    );
    for id in &excluded {
        assert!(
            records.iter().any(|record| &record.push_id == id),
            "excluded push {id:?} must remain in the full held-record API"
        );
        assert!(
            !ids.contains(id),
            "readiness discovered excluded push {id:?}"
        );
    }
    let mut connection = owner.writer.lock().await;
    // This is the same file consumed by the production checked query_file! macro.
    const PLAN: &str = concat!("EXPLAIN QUERY PLAN ", include_str!("held_mail_ids.sql"));
    let plan = sqlx::query(PLAN)
        .bind(String::from(target.endpoint.service_id.clone()))
        .bind(String::from(target.endpoint.endpoint_id.clone()))
        .bind(String::from(target.session_id.clone()))
        .fetch_all(&mut *connection)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String, _>("detail").unwrap())
        .collect::<Vec<_>>();
    assert!(
        plan.iter().any(|detail| {
            let detail = detail.to_ascii_lowercase();
            detail.starts_with("search p ")
                && detail.contains("index router_pushes_held_mail_readiness ")
        }),
        "readiness did not search the target index: {plan:?}"
    );
    for alias in ["r", "poll"] {
        assert!(
            plan.iter().any(|detail| {
                let detail = detail.to_ascii_lowercase();
                detail.starts_with(&format!("search {alias} "))
                    && detail.contains("index ")
                    && detail.contains("(item_id=?)")
            }),
            "readiness did not search {alias} by item ID: {plan:?}"
        );
    }
    assert!(
        plan.iter()
            .all(|detail| !detail.to_ascii_lowercase().contains("temp b-tree")),
        "readiness sorted outside its target index: {plan:?}"
    );
    eprintln!(
        "250 x 64 KiB projections: ID read {identity_read:?}; full-record read {record_read:?}; plan {plan:?}"
    );
    // Readiness validates IDs. Full-record callers retain their complete domain validation.
    let corrupt = expected.first().unwrap();
    sqlx::query("UPDATE router_pushes SET body=? WHERE push_id=?")
        .bind("x".repeat(65_537))
        .bind(corrupt.as_str())
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);
    assert_eq!(owner.held_mail_push_ids(&target).await.unwrap(), expected);
    assert!(owner.list_held_mail_push_records(&target).await.is_err());
    owner.close().await.unwrap();
}

#[tokio::test]
async fn readiness_index_shape_and_ledger_are_checked_on_reopen() {
    for statement in [
        "DROP INDEX router_pushes_held_mail_readiness",
        "DROP INDEX router_pushes_held_mail_readiness; CREATE INDEX router_pushes_held_mail_readiness ON router_pushes(push_id)",
        "DELETE FROM _sqlx_migrations WHERE version=202610210002",
        "DROP INDEX local_delivery_retries_readiness",
        "DROP INDEX local_delivery_retries_readiness; CREATE INDEX local_delivery_retries_readiness ON local_delivery_retries(item_id)",
        "DELETE FROM _sqlx_migrations WHERE version=202610210003",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        InteractionStore::open(&path)
            .await
            .unwrap()
            .close()
            .await
            .unwrap();
        let mut connection = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        sqlx::raw_sql(statement)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(StorageError::InvalidSchema)
            ),
            "readiness index mismatch admitted: {statement}"
        );
    }
}
