//! A real local prevention stays decided when owner takeover discards its group.
use super::*;
use collaboration_protocol::PushDeliveryState;

#[tokio::test]
async fn held_local_follow_keeps_prevention_when_takeover_discards_its_unreplicated_end()
-> TestResult {
    let mut fixture = RuleFixture::open(true).await?;
    let proof = async {
        let notifications = fixture
            .holder
            .notifications
            .as_ref()
            .ok_or("old native owner")?;
        let (release, task) = notifications.hold_progress_writer().await?;
        let seed = fixture.seed_follow(true).await?;
        fixture.confirm_prefix(fixture.head().await?).await?;
        wait_seed_handoff_acknowledgment(&fixture, seed.handoff_sequence).await?;
        release.send(()).map_err(|_| "native progress hold ended")?;
        task.await??;
        notifications
            .publisher
            .queue_project(fixture.project.clone())
            .await?;
        wait_local_state(notifications, seed.item.id(), PushDeliveryState::Held).await?;
        let connection = ObjectAddress::Connection(origin()?.installation);
        fixture
            .holder
            .central
            .runtime_control(
                connection.clone(),
                RuntimeControl::HoldNextDispatch {
                    destination: ObjectAddress::Copy(DatabaseId::try_from(
                        fixture.project.as_str().to_owned(),
                    )?),
                },
            )
            .await?;
        let group = fixture.end_follow(&seed).await?;
        fixture
            .holder
            .central
            .runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch)
            .await?;
        notifications
            .publisher
            .queue_project(fixture.project.clone())
            .await?;
        wait_local_state(notifications, seed.item.id(), PushDeliveryState::Rejected).await?;
        let before = local_progress(notifications, seed.item.id()).await?;
        assert_eq!(before.get("kind"), Some(&json!("final")));
        assert_eq!(
            before.pointer("/value/outcome/kind"),
            Some(&json!("prevented"))
        );
        fixture.holder.disconnect_holder().await?;
        let moved = fixture
            .holder
            .central
            .take_over_project(
                DatabaseId::try_from(fixture.project.as_str().to_owned())?,
                &fixture.successor_origin.installation,
                TransferId::new(durable_steps::Uuid::now_v7())?,
            )
            .await?;
        assert_eq!(moved.pointer("/value/newEpoch"), Some(&json!(3)));
        fixture
            .holder
            .central
            .runtime_control(connection, RuntimeControl::ResumeDispatch)
            .await?;
        let store = received_grants::wait_published_project(
            &fixture.successor.registry,
            &fixture.project,
            3,
        )
        .await?;
        let mut read = store.snapshot().await?;
        assert!(read.mail_handoff(&group.id).await?.is_none());
        assert!(
            read.mail_handoff(&seed.item.id().as_uuid().to_string().try_into()?)
                .await?
                .is_some()
        );
        read.finish().await?;
        let notifications = fixture
            .holder
            .notifications
            .as_ref()
            .ok_or("old native owner")?;
        assert_eq!(local_progress(notifications, seed.item.id()).await?, before);
        wait_local_state(notifications, seed.item.id(), PushDeliveryState::Rejected).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

async fn wait_seed_handoff_acknowledgment(
    fixture: &RuleFixture,
    sequence: HandoffSeq,
) -> TestResult {
    timeout(BOUND, async {
        loop {
            let marks = fixture
                .holder
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?;
            if marks
                .max_handoff_seq
                .is_some_and(|through| through >= sequence.get())
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn local_progress(
    owner: &forwarded_notification_fixture::HolderNotifications,
    item: ItemId,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let rows = owner.rows().await?;
    let row = rows
        .into_iter()
        .find(|row| row.category == "inbox" && row.identity == item.as_uuid().to_string())
        .ok_or("local native custody")?;
    let record: serde_json::Value =
        serde_json::from_str(row.content.as_deref().ok_or("native header")?)?;
    Ok(record.get("progress").ok_or("native progress")?.clone())
}
async fn wait_local_state(
    owner: &forwarded_notification_fixture::HolderNotifications,
    item: ItemId,
    expected: PushDeliveryState,
) -> TestResult {
    let observed = timeout(BOUND, async {
        loop {
            if owner
                .local_push(item)
                .await?
                .is_some_and(|record| record.record.delivery_state == expected)
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    match observed {
        Ok(result) => result,
        Err(error) => {
            let actual = owner
                .local_push(item)
                .await?
                .map(|shown| shown.record.delivery_state);
            Err(format!(
                "local native custody did not reach {expected:?}: {error}; actual={actual:?}"
            )
            .into())
        }
    }
}
