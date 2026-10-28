use crate::ProjectStore;
use message_board::*;
use sqlx::{ConnectOptions, Connection};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn router_expiry_membership_reopens_and_a_non_expiry_router_row_is_refused()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("router-expiry.sqlite");
    let project = ProjectId::generate();
    let base = crate::project_store_tests::project_record(project.clone());
    let reader = base
        .envelope()
        .actor
        .declared_identity()
        .ok_or("fixture reader")?;
    let now = base
        .envelope()
        .time
        .as_router_time()
        .ok_or("fixture time")?;
    let scope = SubscriptionScope::project(project.clone());
    let origin = crate::project_test_support::origin();
    let installation = origin.installation.clone();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store.begin_admission(origin).await?;
    write.append_record(base.clone()).await?;
    let high = write.high_water().await?;
    let cursor = FeedCursor {
        epoch: write.admitted_epoch()?,
        position: high.position.successor()?,
    };
    let member = SubscriptionMembership::new(SubscriptionMembershipProps {
        envelope: RecordEnvelope {
            id: RecordId::generate(),
            actor: RecordedActor::Router {
                component: RouterComponent::SubscriptionDelivery,
                installation,
            },
            request: None,
            position: write.record_placement(cursor),
            ..base.envelope().clone()
        },
        reader: reader.clone(),
        scope: scope.clone(),
        policy: SubscriptionPolicy::defaults_for(&reader),
        renewed_at: now - chrono::Duration::days(1),
        expires_at: now,
        generation: SubscriptionGeneration::new(1)?,
        activated_at: FeedCursor {
            epoch: cursor.epoch,
            position: FeedPosition::ZERO,
        },
        ended: Some(MembershipEnd {
            reason: EndReason::Expired,
            position: cursor,
            at: now,
        }),
    })?;
    write
        .append_record(Record::SubscriptionMembership(member.clone()))
        .await?;
    write.commit().await?;
    store.close().await;

    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.membership(&reader, &scope).await?,
        Some(member.clone())
    );
    read.finish().await?;
    store.close().await;

    // Corrupt only the project-owned membership row; no SDK table is touched.
    let mut invalid_end = member.ended().cloned().ok_or("expiry end")?;
    invalid_end.reason = EndReason::Cancelled;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE subscription_memberships SET ended=? WHERE membership_id=?")
        .bind(serde_json::to_string(&invalid_end)?)
        .bind(member.envelope.id.as_str())
        .execute(&mut connection)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let store = ProjectStore::open(&path, project).await?;
    let mut read = store.snapshot().await?;
    let decoded = read.membership(&reader, &scope).await;
    read.finish().await?;
    store.close().await;
    assert!(
        matches!(
            &decoded,
            Err(crate::ProjectStorageError::InvalidRecord {
                field: "membership",
                ..
            })
        ),
        "a stored Router non-expiry membership was not explicitly refused: {decoded:?}"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn native_membership_survives_reopen_without_local_delivery_fields()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let base = crate::project_store_tests::project_record(project.clone());
    let reader = base
        .envelope()
        .actor
        .declared_identity()
        .ok_or("declared fixture")?
        .clone();
    let scope = SubscriptionScope::project(project.clone());
    let mut membership = SubscriptionMembership::new(SubscriptionMembershipProps {
        envelope: RecordEnvelope {
            id: RecordId::generate(),
            ..base.envelope().clone()
        },
        reader: reader.clone(),
        scope: scope.clone(),
        policy: SubscriptionPolicy::defaults_for(&reader),
        renewed_at: base
            .envelope()
            .time
            .as_router_time()
            .ok_or("fixture Router time")?,
        expires_at: base
            .envelope()
            .time
            .as_router_time()
            .ok_or("fixture Router time")?
            + chrono::Duration::days(1),
        generation: SubscriptionGeneration::new(1)?,
        activated_at: FeedCursor {
            epoch: EpochNumber::try_from(1)?,
            position: FeedPosition::ZERO,
        },
        ended: None,
    })?;
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base).await?;
    membership.envelope.position = crate::project_test_support::placement(
        write
            .append_record(Record::SubscriptionMembership(membership.clone()))
            .await?,
    );
    assert_eq!(
        write.membership(&reader, &scope).await?,
        Some(membership.clone())
    );
    write.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.membership(&reader, &scope).await?,
        Some(membership.clone())
    );
    assert_eq!(
        read.subscription_memberships(Some(&reader))
            .await?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?,
        vec![membership.clone()]
    );
    let payload = serde_json::to_value(membership)?;
    assert!(payload.get("roots").is_none());
    assert!(payload.get("deliveredThrough").is_none());
    assert!(payload.get("lastOutcome").is_none());
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn membership_keyset_pages_keep_reader_scope_and_validate_only_selected_bodies()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("project.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let reader = base
        .envelope()
        .actor
        .declared_identity()
        .ok_or("fixture reader")?
        .clone();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    let mut expected = std::collections::BTreeSet::new();
    let mut corrupt = None;
    for index in 0..107 {
        let thread = ThreadId::generate();
        write
            .append_record(Record::Thread(Thread {
                envelope: RecordEnvelope {
                    id: thread.clone().into(),
                    ..base.envelope().clone()
                },
                thread_id: thread.clone(),
                about: ThreadSubject::Project {
                    line: SubjectLine::try_from("Fixture discussion".to_owned())?,
                },
                imported: false,
                state: ThreadState::Open,
                orchestrator: None,
                implementer: None,
            }))
            .await?;
        let selected_reader = if index < 105 {
            reader.clone()
        } else {
            Identity::Human {
                human_id: HumanId::try_from(format!("other-{index}"))?,
            }
        };
        let id = RecordId::generate();
        if index < 105 {
            expected.insert(id.clone());
        } else if index == 105 {
            corrupt = Some(id.clone());
        }
        let member = SubscriptionMembership::new(SubscriptionMembershipProps {
            envelope: RecordEnvelope {
                id,
                ..base.envelope().clone()
            },
            reader: selected_reader.clone(),
            scope: SubscriptionScope::thread(thread),
            policy: SubscriptionPolicy::defaults_for(&selected_reader),
            renewed_at: base
                .envelope()
                .time
                .as_router_time()
                .ok_or("fixture Router time")?,
            expires_at: base
                .envelope()
                .time
                .as_router_time()
                .ok_or("fixture Router time")?
                + chrono::Duration::days(1),
            generation: SubscriptionGeneration::new(1)?,
            activated_at: FeedCursor {
                epoch: EpochNumber::try_from(1)?,
                position: FeedPosition::ZERO,
            },
            ended: None,
        })?;
        write
            .append_record(Record::SubscriptionMembership(member))
            .await?;
    }
    write.commit().await?;
    let corrupt = corrupt.ok_or("corrupt fixture id")?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    sqlx::query("UPDATE subscription_memberships SET policy = json_set(policy, '$.mode', 'corrupt') WHERE membership_id = ?")
        .bind(corrupt.as_str()).execute(&mut connection).await?;
    connection.close().await?;
    let mut read = store.snapshot().await?;
    let selected = read
        .subscription_memberships(Some(&reader))
        .await?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(selected.len(), 105);
    assert_eq!(
        selected
            .iter()
            .map(|member| member.envelope.id.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert!(selected.iter().all(|member| member.reader() == &reader));
    let discovered = read.subscription_memberships(None).await?;
    assert_eq!(discovered.len(), 107);
    assert_eq!(
        discovered.iter().filter(|member| member.is_ok()).count(),
        106
    );
    assert_eq!(
        discovered.iter().filter(|member| member.is_err()).count(),
        1
    );
    assert!(
        read.record(&RecordRef {
            kind: RecordKind::SubscriptionMembership,
            id: corrupt
        })
        .await
        .is_err()
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}
