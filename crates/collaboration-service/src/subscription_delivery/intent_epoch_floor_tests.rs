//! Ownership changes stand in for M4; the floor and C5 consumer use their real native stores.
use super::*;

async fn next_epoch(fixture: &OwnerFixture, number: u64, holder: &ServiceId) -> FeedCursor {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let read = store.snapshot().await.unwrap();
    let previous = read.high_water();
    read.finish().await.unwrap();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await
        .unwrap();
    sqlx::query("UPDATE writer_epochs SET ended = ? WHERE epoch = ?")
        .bind(serde_json::to_string(&EpochEnd::Released {}).unwrap())
        .bind(i64::try_from(previous.epoch.get()).unwrap())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let epoch = EpochNumber::try_from(number).unwrap();
    let mut write = store.begin_admission().await.unwrap();
    write
        .record_writer_epoch(&WriterEpoch {
            project: fixture.project.clone(),
            number: epoch,
            holder: holder.clone(),
            start_position: previous.position,
            end: None,
        })
        .await
        .unwrap();
    write.commit().await.unwrap();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await
        .unwrap();
    sqlx::query("UPDATE feed_checkpoint SET epoch = ? WHERE singleton = 1")
        .bind(i64::try_from(number).unwrap())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    fixture
        .admission
        .leases
        .set_grant(
            fixture.project.clone(),
            crate::GrantResult::Holder {
                epoch,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(3600),
            },
        )
        .await;
    FeedCursor {
        epoch,
        position: previous.position,
    }
}

async fn append_deferred_intent(
    fixture: &OwnerFixture,
    template: &NotifyIntentId,
    holder: &ServiceId,
) -> NotifyIntentId {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut write = store.begin_admission().await.unwrap();
    let Some(Record::NotifyIntent(template)) =
        write.record(&template.clone().into()).await.unwrap()
    else {
        panic!("template intent");
    };
    let mut props: NotifyIntentProps = template.into();
    let Some(Record::Post(mut cause)) = write.record(&props.cause).await.unwrap() else {
        panic!("template cause");
    };
    cause.post_id = PostId::generate();
    cause.envelope.id = cause.post_id.clone().into();
    cause.envelope.request = None;
    cause.envelope.actor.installation = holder.clone();
    props.cause = cause.post_id.clone().into();
    write.append_record(Record::Post(cause)).await.unwrap();
    props.intent_id = NotifyIntentId::generate();
    props.envelope.id = props.intent_id.clone().into();
    props.envelope.request = None;
    props.envelope.actor.installation = holder.clone();
    let intent = NotifyIntent::try_from(props).unwrap();
    let id = intent.intent_id.clone();
    write
        .append_record(Record::NotifyIntent(intent))
        .await
        .unwrap();
    write.commit().await.unwrap();
    id
}

async fn assert_emitted(fixture: &OwnerFixture, intents: &[NotifyIntentId]) {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    for id in intents {
        let Some(Record::NotifyIntent(intent)) = read.record(&id.clone().into()).await.unwrap()
        else {
            panic!("intent");
        };
        assert!(matches!(intent.state(), IntentState::Emitted {}));
    }
    read.finish().await.unwrap();
}

#[tokio::test]
async fn cursor_loss_starts_at_the_ownership_floor_and_preserves_inherited_intents() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    let local = fixture.admission.installation.clone();
    let foreign = ServiceId::try_from("550e8400-e29b-41d4-a716-446655440099".to_owned()).unwrap();
    let inherited = addressed_post(&fixture, human("attention")).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await
        .unwrap();
    sqlx::query("UPDATE writer_epochs SET holder = ? WHERE epoch = 1")
        .bind(foreign.as_str())
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let floor = next_epoch(&fixture, 2, &local).await;
    let fresh = append_deferred_intent(&fixture, &inherited, &local).await;
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert!(
        !fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &inherited,
                current_intent_cursor(&fixture, &inherited).await
            )
            .await
            .unwrap()
    );
    assert!(
        !fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &fresh,
                current_intent_cursor(&fixture, &fresh).await
            )
            .await
            .unwrap()
    );
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(read.intent_scan_floor(&local).await.unwrap(), Some(floor));
    let before = read.high_water();
    read.finish().await.unwrap();
    measured_pass(&runtime.service, &fixture).await;
    assert!(
        !fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &inherited,
                current_intent_cursor(&fixture, &inherited).await
            )
            .await
            .unwrap()
    );
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &fresh,
                current_intent_cursor(&fixture, &fresh).await
            )
            .await
            .unwrap()
    );
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert_eq!(measured_pass(&runtime.service, &fixture).await.records, 0);
    assert_emitted(&fixture, &[inherited, fresh]).await;
    let read = store.snapshot().await.unwrap();
    assert_eq!(
        read.high_water(),
        before,
        "Deferred handling changed public project state"
    );
    read.finish().await.unwrap();
    runtime.close().await;
}

#[tokio::test]
async fn returning_holder_gets_a_new_floor_across_its_unbroken_epoch_run() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    let local = fixture.admission.installation.clone();
    let foreign = ServiceId::try_from("550e8400-e29b-41d4-a716-446655440099".to_owned()).unwrap();
    let earlier_local = addressed_post(&fixture, human("attention")).await;
    next_epoch(&fixture, 2, &foreign).await;
    let inherited = append_deferred_intent(&fixture, &earlier_local, &foreign).await;
    let floor = next_epoch(&fixture, 3, &local).await;
    let fresh = append_deferred_intent(&fixture, &earlier_local, &local).await;
    next_epoch(&fixture, 4, &local).await;
    let latest = append_deferred_intent(&fixture, &earlier_local, &local).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(read.intent_scan_floor(&local).await.unwrap(), Some(floor));
    let before = read.high_water();
    read.finish().await.unwrap();
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    measured_pass(&runtime.service, &fixture).await;
    for id in [&earlier_local, &inherited] {
        assert!(
            !fixture
                .push_store
                .lock()
                .await
                .intent_consumed(
                    &fixture.project,
                    id,
                    current_intent_cursor(&fixture, id).await
                )
                .await
                .unwrap()
        );
    }
    for id in [&fresh, &latest] {
        assert!(
            fixture
                .push_store
                .lock()
                .await
                .intent_consumed(
                    &fixture.project,
                    id,
                    current_intent_cursor(&fixture, id).await
                )
                .await
                .unwrap()
        );
    }
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert_eq!(measured_pass(&runtime.service, &fixture).await.records, 0);
    assert_emitted(&fixture, &[earlier_local, inherited, fresh, latest]).await;
    let read = store.snapshot().await.unwrap();
    assert_eq!(read.high_water(), before);
    read.finish().await.unwrap();
    runtime.close().await;
}
