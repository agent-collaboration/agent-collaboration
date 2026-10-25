//! Follow outcome recovery replays the saved native answer inside the real SDK drive.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use sqlx::Connection;

#[tokio::test]
async fn a_failed_follow_projection_keeps_one_frozen_item_for_the_next_project_hint() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_follow_projection BEFORE INSERT ON router_pushes BEGIN SELECT RAISE(ABORT, 'injected follow projection insert fault'); END")
        .execute(&mut observer).await.unwrap();
    fixture
        .post(
            &fixture.root,
            "one frozen allocation survives native admission failure",
        )
        .await;
    let frozen = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frozen = frozen_follow(&fixture).await;
            if !frozen.is_empty() {
                break frozen;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await
    .unwrap();
    assert_eq!(frozen.len(), 1);
    let original = serde_json::to_value(&frozen[0]).unwrap();
    let push: collaboration_protocol::PushId =
        frozen[0].id().as_uuid().to_string().try_into().unwrap();
    assert!(
        fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .runtime()
            .local_outcome_for_test(frozen[0].id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .runtime()
            .local_custody_is_absent_for_test(DeliveryItemRef {
                item: frozen[0].id(),
                digest: frozen[0].digest().clone(),
            })
            .await
            .unwrap(),
        "failed native projection admitted SDK custody"
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let native_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM router_pushes")
        .fetch_one(&mut observer)
        .await
        .unwrap();
    assert_eq!(native_rows, 0);

    sqlx::query("DROP TRIGGER fail_follow_projection")
        .execute(&mut observer)
        .await
        .unwrap();
    let board_sync_protocol::BoardDeliveryBody::Notification(notification) = frozen[0].body()
    else {
        panic!("real frozen notification");
    };
    let collaboration_mail_protocol::NotificationSource::FollowBatch {
        membership,
        activated_at,
        generation,
        roots,
        ..
    } = &notification.source
    else {
        panic!("real follow source");
    };
    // A reader's stale same-range proposal races recovery through the real
    // locked admission port. It must not mint a second immutable handoff.
    let stale = fixture
        .admission
        .allocate_follow_batch(
            &fixture.project,
            crate::project_admission::FollowBatchProposal {
                membership: membership.as_str().to_owned().try_into().unwrap(),
                activated_at: *activated_at,
                generation: *generation,
                roots: roots.clone(),
                item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap(),
                enrollment: crate::IntentRecipientEnrollment {
                    service: frozen[0].recipient().address.endpoint.service_id.clone(),
                    peer: frozen[0].recipient().peer,
                },
                preventions: Vec::new(),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    if stale.is_some() {
        let duplicated = frozen_follow(&fixture).await.len();
        publisher.shutdown().await;
        drop(publisher);
        observer.close().await.unwrap();
        drop(app);
        runtime.close().await;
        fixture.close_native(native).await;
        assert_eq!(duplicated, 1, "recovery allocated a second frozen item");
        panic!("stale recovery proposal was admitted");
    }

    // Existing ACK/Welcome/activity callers use this same hint; no timer or
    // invented Selected event substitutes for the publisher's real retry.
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.payload.push_id, push);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    fixture
        .await_push_state(&push, PushDeliveryState::Delivered)
        .await;
    tokio::time::timeout(
        Duration::from_secs(10),
        publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await
    .unwrap();
    let shown = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: request.target,
            reference: push.as_str().to_owned(),
        })
        .await
        .unwrap();
    let final_frozen = frozen_follow(&fixture).await;
    let sdk = native
        .runtime()
        .local_outcome_for_test(frozen[0].id())
        .await
        .unwrap();
    let calls = runtime
        .completed_native_calls
        .load(std::sync::atomic::Ordering::SeqCst);
    publisher.shutdown().await;
    drop(publisher);
    observer.close().await.unwrap();
    drop(app);
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
    fixture.close_native(native).await;
    assert_eq!(calls, 1);
    assert!(matches!(
        sdk,
        Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(shown.record.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        shown.activity_ranges[0].posts[0].text.as_str(),
        "one frozen allocation survives native admission failure"
    );
    assert_eq!(final_frozen.len(), 1);
    assert_eq!(serde_json::to_value(&final_frozen[0]).unwrap(), original);
}

#[tokio::test]
async fn a_follow_outcome_write_fault_recovers_without_a_second_native_call() {
    // Arrange the real project producer, publisher, SDK and reader carrier.
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_follow_outcome BEFORE UPDATE OF delivery_state ON router_pushes WHEN OLD.delivery_state='attempted' AND NEW.delivery_state='delivered' BEGIN SELECT RAISE(ABORT, 'injected follow outcome fault'); END")
        .execute(&mut observer).await.unwrap();
    fixture
        .post(
            &fixture.root,
            "saved follow answer survives its failed outcome transaction",
        )
        .await;
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    let show = PushRecordShowParams {
        caller: request.target,
        reference: request.payload.push_id.as_str().to_owned(),
    };
    let before = app.messages().push_show(show.clone()).await.unwrap();
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    assert_eq!(
        frozen[0].id().as_uuid().to_string(),
        request.payload.push_id.as_str()
    );

    // Act: the actual client accepts once, then the real outcome transaction fails.
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await
    .expect("publisher's first settlement drive must finish its failed attempt");
    let failed = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.delivery_state, PushDeliveryState::Attempted);
    assert!(
        native
            .runtime()
            .local_outcome_for_test(frozen[0].id())
            .await
            .unwrap()
            .is_none(),
        "failed native projection committed SDK custody anyway"
    );
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // Repair only the injected fault; the existing SDK retry must use its saved answer.
    sqlx::query("DROP TRIGGER fail_follow_outcome")
        .execute(&mut observer)
        .await
        .unwrap();
    fixture.clock.advance_without_tokio_time(100);
    let mail = native.runtime_handle();
    let driving = tokio::spawn(async move { mail.drive_due_for_test().await });
    let terminal = async {
        loop {
            let record = fixture
                .interactions
                .get_push_record(&request.payload.push_id)
                .await
                .unwrap()
                .unwrap();
            if record.settled_at.is_some() {
                break record;
            }
            tokio::task::yield_now().await;
        }
    };
    let mut duplicate_native_call = false;
    let terminal = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::pin!(terminal);
        tokio::select! {
            record = &mut terminal => record,
            _request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                duplicate_native_call = true;
                runtime.completions.send(DeliveryOutcome::Started).unwrap();
                terminal.await
            }
        }
    })
    .await;
    tokio::time::timeout(Duration::from_secs(10), driving)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let after = app.messages().push_show(show).await.unwrap();
    let sdk = native
        .runtime()
        .local_outcome_for_test(frozen[0].id())
        .await
        .unwrap();
    let final_frozen = frozen_follow(&fixture).await;
    publisher.shutdown().await;
    drop(publisher);
    observer.close().await.unwrap();
    drop(app);
    let calls = runtime
        .completed_native_calls
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
    fixture.close_native(native).await;

    assert!(
        !duplicate_native_call,
        "outcome recovery dispatched the saved answer again"
    );
    assert_eq!(calls, 1);
    assert_eq!(
        terminal
            .expect("follow outcome retry never committed")
            .delivery_state,
        PushDeliveryState::Delivered
    );
    assert!(matches!(
        sdk,
        Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(after.record.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(after.link, before.link);
    assert_eq!(
        serde_json::to_value(after.activity_ranges).unwrap(),
        serde_json::to_value(before.activity_ranges).unwrap()
    );
    assert_eq!(final_frozen.len(), 1);
    assert_eq!(
        serde_json::to_value(&final_frozen[0]).unwrap(),
        serde_json::to_value(&frozen[0]).unwrap()
    );
}
