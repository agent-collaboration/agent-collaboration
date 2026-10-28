//! Native handoff rows survive owning reopen and refuse altered frozen identities.
// Each literal page index follows its asserted length.
use crate::{MailHandoffItem, ProjectStorageError, ProjectStore, project_test_support::origin};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    Excerpt, MailSender, NotificationBody, NotificationSource, RecordPointer,
};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, Recipient};
use message_board::*;
use sqlx::{ConnectOptions, Connection, Row};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn terminal_item_lookup_uses_the_exact_indexed_handoff_query_and_detects_a_scan() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(
        &directory.path().join("handoff-plan.sqlite"),
        project.clone(),
    )
    .await?;
    let handoff = notification(
        &project,
        ItemId::new(uuid::Uuid::now_v7())?,
        "indexed terminal lookup",
    )?;
    let id = handoff.id()?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project))
        .await?;
    write.append_mail_handoff(&handoff).await?;
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.mail_handoff(&id)
            .await?
            .ok_or("missing handoff")?
            .item
            .id()?,
        id
    );
    read.finish().await?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    let production = include_str!("project_mail_handoff.sql");
    let mut plans = Vec::new();
    for query in [
        production.to_owned(),
        production.replace("WHERE item_id=?", "WHERE item_id || ''=?"),
    ] {
        // Only the compiled-in production SQL and one literal predicate change;
        // the item identity remains a bound parameter in both plans.
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {query}")))
            .bind(id.as_str())
            .fetch_all(&mut connection)
            .await?;
        plans.push(
            rows.iter()
                .map(|row| row.try_get::<String, _>("detail"))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    connection.close().await?;
    store.close().await;
    let indexed = |details: &[String]| {
        details
            .iter()
            .any(|row| row.contains("SEARCH") && row.contains("INDEX"))
            && !details.iter().any(|row| row.contains("SCAN"))
    };
    let [correct, wrong] = plans.as_slice() else {
        return Err("two query plans".into());
    };
    eprintln!("HANDOFF_POINT_LOOKUP_PLAN {correct:?}; WRONG_SCAN_PLAN {wrong:?}");
    assert!(
        indexed(correct),
        "terminal hint lookup did not use an indexed SEARCH: {correct:?}"
    );
    assert!(
        !indexed(wrong) && wrong.iter().any(|row| row.contains("SCAN")),
        "wrong query did not expose its SCAN: {wrong:?}"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn frontier_writes_are_monotonic_atomic_feed_free_and_survive_owner_reopen() -> TestResult {
    // Arrange two immutable handoffs in the actual owning writer.
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("frontier-writes.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    for text in ["first", "second"] {
        write
            .append_mail_handoff(&notification(
                &project,
                ItemId::new(uuid::Uuid::now_v7())?,
                text,
            )?)
            .await?;
    }
    let feed = write.high_water().await?;
    write.commit().await?;
    let mut write = store.begin_admission(origin()).await?;
    assert!(write.advance_mail_settled_through(1.try_into()?).await?);
    write.rollback().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.mail_settled_through().await?, HandoffSeq::ZERO);
    assert_eq!(read.mail_handoff_head().await?.get(), 2);
    read.finish().await?;

    // Act: the committed prefix changes no feed position; stale and future writes refuse.
    let mut write = store.begin_admission(origin()).await?;
    assert!(write.advance_mail_settled_through(1.try_into()?).await?);
    assert_eq!(write.high_water().await?, feed);
    write.commit().await?;
    for invalid in [0_u64, 3] {
        let mut write = store.begin_admission(origin()).await?;
        assert!(matches!(
            write
                .advance_mail_settled_through(invalid.try_into()?)
                .await,
            Err(ProjectStorageError::InvalidFeed)
        ));
        write.rollback().await?;
    }
    let mut write = store.begin_admission(origin()).await?;
    assert!(!write.advance_mail_settled_through(1.try_into()?).await?);
    write.rollback().await?;
    let mut write = store.begin_admission(origin()).await?;
    assert!(write.advance_mail_settled_through(2.try_into()?).await?);
    write.commit().await?;
    store.close().await;

    // Assert the final singleton and original handoffs after physical owner reopen.
    let store = ProjectStore::open(&path, project).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.mail_settled_through().await?.get(), 2);
    assert_eq!(read.mail_handoff_page(HandoffSeq::ZERO, 10).await?.len(), 2);
    assert_eq!(read.high_water(), feed);
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn settled_frontier_reopens_and_refuses_ambiguous_or_impossible_stored_rows() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("frontier.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.mail_settled_through().await?, HandoffSeq::ZERO);
    read.finish().await?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    write
        .append_mail_handoff(&notification(
            &project,
            ItemId::new(uuid::Uuid::now_v7())?,
            "settled original",
        )?)
        .await?;
    write.commit().await?;
    let key = uuid::Uuid::now_v7().to_string();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    sqlx::query("INSERT INTO handoff_frontier(frontier_id,settled_through) VALUES(?,1)")
        .bind(&key)
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.mail_settled_through().await?.get(), 1);
    read.finish().await?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    // The row decoder owns UUID, singleton and range validation, not SQL CHECK clauses.
    for (id, position) in [
        ("550e8400-e29b-41d4-a716-446655440000".to_owned(), 1_i64),
        (key.to_uppercase(), 1),
        (key.clone(), -1),
        (key.clone(), 2),
    ] {
        sqlx::query("UPDATE handoff_frontier SET frontier_id=?,settled_through=?")
            .bind(&id)
            .bind(position)
            .execute(&mut connection)
            .await?;
        let mut read = store.snapshot().await?;
        assert!(
            matches!(
                read.mail_settled_through().await,
                Err(ProjectStorageError::InvalidFeed)
            ),
            "accepted invalid frontier id={id}, position={position}"
        );
        read.finish().await?;
        sqlx::query("UPDATE handoff_frontier SET frontier_id=?,settled_through=1")
            .bind(&key)
            .execute(&mut connection)
            .await?;
    }
    sqlx::query("INSERT INTO handoff_frontier(frontier_id,settled_through) VALUES(?,1)")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut connection)
        .await?;
    let mut read = store.snapshot().await?;
    assert!(
        matches!(
            read.mail_settled_through().await,
            Err(ProjectStorageError::InvalidFeed)
        ),
        "multiple frontier identities were silently combined"
    );
    read.finish().await?;
    connection.close().await?;
    store.close().await;
    Ok(())
}

fn notification(
    project: &ProjectId,
    id: ItemId,
    excerpt: &str,
) -> Result<MailHandoffItem, Box<dyn std::error::Error + Send + Sync>> {
    Ok(MailHandoffItem::Delivery(Box::new(DeliveryEnvelope::Item(
        DeliveryItem::<BoardDeliveryTypes>::new(
            id,
            MailSender::Project {
                project: project.clone(),
            },
            Recipient {
                peer: PeerId::new(uuid::Uuid::now_v7())?,
                address: SessionRef {
                    endpoint: SessionEndpointRef {
                        service_id: origin().installation,
                        endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
                    },
                    session_id: SessionId::try_from("handoff-recipient".to_owned())?,
                },
            },
            None,
            BoardDeliveryBody::Notification(NotificationBody {
                excerpt: Excerpt::try_from(excerpt.to_owned())?,
                pointer: RecordPointer {
                    project: project.clone(),
                    subject: project.clone().into(),
                    position: FeedCursor {
                        epoch: EpochNumber::INITIAL,
                        position: FeedPosition::try_from(1)?,
                    },
                },
                source: NotificationSource::Intent {
                    intent: NotifyIntentId::generate(),
                },
            }),
        )?,
    ))))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn frozen_handoffs_commit_in_transaction_order_replay_exactly_and_survive_reopen()
-> TestResult {
    // Arrange: UUID order deliberately opposes allocation order.
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("frozen-handoffs.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let first = notification(
        &project,
        ItemId::new(uuid::Uuid::parse_str(
            "01900000-0000-7000-8000-000000000002",
        )?)?,
        "first",
    )?;
    let second = notification(
        &project,
        ItemId::new(uuid::Uuid::parse_str(
            "01900000-0000-7000-8000-000000000001",
        )?)?,
        "second",
    )?;
    let expected_first = serde_json::to_value(&first)?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    let head = write.high_water().await?;

    // Act: allocation and its cause use one native transaction, with no feed row for the handoff.
    assert_eq!(write.append_mail_handoff(&first).await?.get(), 1);
    assert_eq!(write.append_mail_handoff(&first).await?.get(), 1);
    assert_eq!(write.append_mail_handoff(&second).await?.get(), 2);
    assert_eq!(write.high_water().await?, head);
    write.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project.clone()).await?;

    // Assert: owning reopen retains complete SDK content and its single identity/digest.
    let mut read = store.snapshot().await?;
    let first_page = read.mail_handoff_page(HandoffSeq::ZERO, 1).await?;
    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page[0].sequence.get(), 1);
    assert_eq!(serde_json::to_value(&first_page[0].item)?, expected_first);
    let second_page = read.mail_handoff_page(first_page[0].sequence, 1).await?;
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].item.id()?, second.id()?);
    assert_eq!(second_page[0].sequence.get(), 2);
    read.finish().await?;
    let MailHandoffItem::Delivery(envelope) = &first else {
        return Err("delivery".into());
    };
    let conflicting = notification(&project, envelope.id(), "changed under original identity")?;
    let mut write = store.begin_admission(origin()).await?;
    assert!(matches!(
        write.append_mail_handoff(&conflicting).await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    write.rollback().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        serde_json::to_value(
            read.mail_handoff(&first.id()?)
                .await?
                .ok_or("original handoff")?
                .item
        )?,
        expected_first
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn rolled_back_handoff_and_its_cause_leave_no_allocation_or_sequence_gap() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(
        &directory.path().join("rollback-handoff.sqlite"),
        project.clone(),
    )
    .await?;
    let item = notification(&project, ItemId::new(uuid::Uuid::now_v7())?, "rolled back")?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    assert_eq!(write.append_mail_handoff(&item).await?.get(), 1);
    write.rollback().await?;
    let mut read = store.snapshot().await?;
    assert!(read.record(&project.clone().into()).await?.is_none());
    assert!(read.mail_handoff(&item.id()?).await?.is_none());
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    assert_eq!(write.append_mail_handoff(&item).await?.get(), 1);
    write.commit().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn stored_handoff_decode_refuses_bad_keys_sequences_envelope_identities_and_digests()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(
        &directory.path().join("invalid-handoff.sqlite"),
        project.clone(),
    )
    .await?;
    let item = notification(&project, ItemId::new(uuid::Uuid::now_v7())?, "original")?;
    let mut write = store.begin_admission(origin()).await?;
    write
        .append_record(crate::project_store_tests::project_record(project.clone()))
        .await?;
    write.append_mail_handoff(&item).await?;
    write.commit().await?;
    let original = serde_json::to_value(&item)?;
    let mut invalid_digest = original.clone();
    invalid_digest["value"]["value"]["body"]["excerpt"] = "altered body without digest".into();
    let another = notification(
        &project,
        ItemId::new(uuid::Uuid::now_v7())?,
        "other identity",
    )?;
    let foreign_project = notification(
        &ProjectId::generate(),
        ItemId::new(uuid::Uuid::now_v7())?,
        "foreign",
    )?;
    let mut corrupt = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    for (key, ordinal, json) in [
        (
            "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            1_i64,
            original.clone(),
        ),
        (item.id()?.as_str().to_owned(), 0, original.clone()),
        (item.id()?.as_str().to_owned(), -1, original.clone()),
        (item.id()?.as_str().to_owned(), 1, invalid_digest),
        (
            item.id()?.as_str().to_owned(),
            1,
            serde_json::to_value(another)?,
        ),
        (
            item.id()?.as_str().to_owned(),
            1,
            serde_json::to_value(foreign_project)?,
        ),
    ] {
        // A damaged file can bypass the native writer's key-change guard. Exercise
        // the decoder with that file, using the same direct native corruption seam
        // as the other storage integrity tests.
        sqlx::query("UPDATE mail_handoffs SET item_id=?,handoff_seq=?,item=?")
            .bind(&key)
            .bind(ordinal)
            .bind(serde_json::to_string(&json)?)
            .execute(&mut corrupt)
            .await?;
        let mut read = store.snapshot().await?;
        // Read the key, not only a positive-ordinal page that could hide a corrupt row.
        let reference = MailItemRef::try_from(key.clone());
        if let Ok(reference) = reference {
            assert!(
                matches!(
                    read.mail_handoff(&reference).await,
                    Err(ProjectStorageError::InvalidFeed)
                ),
                "accepted ordinal={ordinal}, json={json}"
            );
        } else {
            assert!(matches!(
                read.mail_handoff_page(HandoffSeq::ZERO, 10).await,
                Err(ProjectStorageError::InvalidFeed)
            ));
        }
        read.finish().await?;
        sqlx::query("UPDATE mail_handoffs SET item_id=?,handoff_seq=1,item=?")
            .bind(item.id()?.as_str())
            .bind(serde_json::to_string(&original)?)
            .execute(&mut corrupt)
            .await?;
    }
    corrupt.close().await?;
    store.close().await;
    Ok(())
}
