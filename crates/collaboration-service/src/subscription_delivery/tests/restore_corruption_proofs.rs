//! One corrupt recovery row must not suppress healthy readers at Host startup.
use super::super::{
    SubscriptionClock, subscription_push::target_session, subscription_service::OwnerObservation,
};
use super::owner_fixture::*;
use collaboration_protocol::{
    MessageDelivery, PushDeliveryState, PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecord,
    PushRecordDraft, SessionId, SessionRef,
};
use message_board::*;
use sqlx::Connection;
use std::time::Duration;

fn direct_message_draft(
    target: &SessionRef,
    push_body: &str,
    created_at: chrono::DateTime<chrono::Utc>,
) -> PushRecordDraft {
    PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: target.clone(),
        mode: Some(MessageDelivery::Auto),
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some(push_body.to_owned()),
        activity: None,
        created_at,
    }
}

async fn stored_push(fixture: &OwnerFixture, push_id: &PushId) -> PushRecord {
    fixture
        .interactions
        .get_push_record(push_id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn shared_progress_store_failure_is_not_contained_as_a_project_failure() {
    let fixture = OwnerFixture::new().await;
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("ALTER TABLE subscription_delivery_progress RENAME TO unavailable_progress")
        .execute(&mut observer)
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .lock()
            .await
            .restore_active_and_draining(fixture.clock.now())
            .await
            .is_err(),
        "shared native message failure must still fail subscription startup"
    );
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    project.snapshot().await.unwrap().finish().await.unwrap();
    observer.close().await.unwrap();
}

#[tokio::test]
async fn corrupt_subscription_mode_is_skipped_while_valid_reader_restores_and_delivers() {
    tokio::time::timeout(Duration::from_secs(30), async {
    let fixture = OwnerFixture::new().await;
    let corrupt_reader = human("corrupt-reader");
    fixture
        .join_request(
            ThreadJoinRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                thread: (fixture.root.clone()).into(),
                actor: corrupt_reader.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: None,
                when_idle: None,
                replace: None,
                note: None,
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    fixture
        .subscribe(
            ThreadSubscriptionSubscribeRequest {
                reader: corrupt_reader.clone(),
                scope: SubscriptionScope::thread(fixture.root.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
                    ..SubscriptionPolicyPatch::default()
                },
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    let native = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = native.snapshot().await.unwrap();
    let membership = read
        .membership(
            &corrupt_reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    let membership_id = membership.envelope.id.as_str();
    use sqlx::ConnectOptions;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(native.path())
        .connect()
        .await
        .unwrap();
    sqlx::query("PRAGMA capture_data_changes_conn('full')")
        .execute(&mut observer)
        .await
        .unwrap();
    fixture
        .post(&fixture.root, "Healthy reader pending at restart")
        .await;
    let payload: String =
        sqlx::query_scalar("SELECT policy FROM subscription_memberships WHERE membership_id = ?")
            .bind(membership_id)
            .fetch_one(&mut observer)
            .await
            .unwrap();
    let mut encoded: serde_json::Value = serde_json::from_str(&payload).unwrap();
    encoded["mode"] = serde_json::json!("corrupt-mode");
    let corrupt_payload = serde_json::to_string(&encoded).unwrap();
    assert_eq!(
        sqlx::query("UPDATE subscription_memberships SET policy = ? WHERE membership_id = ?",)
            .bind(&corrupt_payload)
            .bind(membership_id)
            .execute(&mut observer)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    // Compare the complete typed row, rather than only the corrupted policy column.
    let membership_row_sql = "SELECT json_array(membership_id,actor_id,request_id,epoch,position,router_time,time_basis,reader_actor,scope_kind,scope_id,policy,renewed_at,expires_at,generation,activated_epoch,activated_position,ended) FROM subscription_memberships WHERE membership_id=?";
    let corrupt_row: String = sqlx::query_scalar(membership_row_sql)
        .bind(membership_id)
        .fetch_one(&mut observer)
        .await
        .unwrap();
    let runtime = fixture.native_runtime().await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("1 message"));
    runtime.await_settled().await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert!(
        fixture
            .store
            .lock()
            .await
            .get_thread_subscription_record(
                &fixture.reader,
                &SubscriptionScope::thread(fixture.root.clone()),
                fixture.clock.now()
            )
            .await
            .unwrap()
            .unwrap()
            .roots()
            .is_empty()
    );
    let corrupt = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(
            &corrupt_reader,
            &SubscriptionScope::thread(fixture.root.clone()),
            fixture.clock.now(),
        )
        .await
        .unwrap_err();
    assert_eq!(corrupt.kind, BoardFailureKind::InvalidRecord);
    assert!(
        corrupt
            .message
            .contains("subscriptionMembership.policy.mode")
    );
    assert!(
        matches!(&corrupt.details, message_board::BoardErrorDetails::FieldConstraint { field,.. } if field=="subscriptionMembership.policy.mode")
    );
    let stored_payload: String = sqlx::query_scalar(membership_row_sql)
        .bind(membership_id)
        .fetch_one(&mut observer)
        .await
        .unwrap();
    assert_eq!(
        stored_payload, corrupt_row,
        "corrupt native membership row must not be silently repaired"
    );
    observer.close().await.unwrap();
    runtime.close().await;
    }).await.expect("bounded healthy native delivery and unchanged corrupt membership");
}

async fn bounded_stage<TValue>(
    stage: &str,
    work: impl std::future::Future<Output = TValue>,
) -> TValue {
    eprintln!("recovery proof: {stage}");
    tokio::time::timeout(Duration::from_secs(10), work)
        .await
        .unwrap_or_else(|_| panic!("recovery proof timed out during {stage}"))
}

#[tokio::test]
async fn failed_direct_message_recovery_query_does_not_stop_valid_subscription_reader() {
    let fixture = bounded_stage("create fixture", OwnerFixture::new()).await;
    bounded_stage(
        "post healthy subscription activity",
        fixture.post(&fixture.root, "Healthy reader despite DM query failure"),
    )
    .await;
    let mut observer = bounded_stage(
        "open native message observer",
        sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new()
                .filename(fixture.directory.path().join("interaction.sqlite")),
        ),
    )
    .await
    .unwrap();
    // The connection remains usable; only the DM recovery repository query is unavailable.
    bounded_stage(
        "inject recovery query failure",
        sqlx::query("ALTER TABLE router_pushes RENAME COLUMN kind TO temporarily_unavailable_kind")
            .execute(&mut observer),
    )
    .await
    .unwrap();
    let recovery_error = bounded_stage("verify recovery query failure", async {
        fixture.interactions.direct_message_recovery_targets().await
    })
    .await
    .unwrap_err();
    assert!(
        matches!(recovery_error,
        interaction_storage::StorageError::Database(sqlx::Error::Database(ref error))
            if error.code().as_deref() == Some("SQLITE_ERROR") && error.message().contains("no such column")),
        "injection must fail the repository query without converting it to a successful empty result"
    );
    let runtime = bounded_stage(
        "start service with unavailable creation-age kind column",
        fixture.native_runtime(),
    )
    .await;
    bounded_stage(
        "observe owner query-failure retry boundary",
        runtime.observe(|event| matches!(event, OwnerObservation::Sleeping(Some(_)))),
    )
    .await;
    bounded_stage(
        "restore push kind column",
        sqlx::query("ALTER TABLE router_pushes RENAME COLUMN temporarily_unavailable_kind TO kind")
            .execute(&mut observer),
    )
    .await
    .unwrap();
    fixture.clock.advance_without_tokio_time(1);
    let request = bounded_stage("receive healthy subscription notice", async {
        let mut requests = runtime.requests.lock().await;
        let mut observations = runtime.observations.lock().await;
        loop {
            tokio::select! {
                request = requests.recv() => break request.unwrap(),
                event = observations.recv() => eprintln!("recovery proof owner: {:?}", event.unwrap()),
            }
        }
    }).await;
    assert!(request.payload.line.as_str().contains("1 message"));
    // Delivery has already been admitted. The owner serializes its receipt
    // before commands, so the scripted recipient must finish before this test
    // awaits Reconcile. Retain and join the task; never detach it.
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let reconcile = tokio::spawn(async move { service.reconcile_reader(reader).await });
    bounded_stage(
        "settle healthy subscription notice",
        runtime.await_settled(),
    )
    .await;
    bounded_stage("acknowledge reconcile after delivery completes", reconcile)
        .await
        .unwrap()
        .unwrap();
    bounded_stage("close automation observer", observer.close())
        .await
        .unwrap();
    bounded_stage("shut down service", runtime.close()).await;
}

#[tokio::test]
async fn corrupt_recovery_target_row_does_not_strand_held_dm_after_restart() {
    let fixture = OwnerFixture::new().await;
    let target = target_session(&fixture.reader).unwrap();
    let held_draft = direct_message_draft(&target, "held DM survives restart", fixture.clock.now());
    let held_push_id = held_draft.push_id.clone();
    fixture
        .interactions
        .insert_push_record(held_draft)
        .await
        .unwrap();
    fixture
        .interactions
        .hold_unsent_direct_message(
            &held_push_id,
            collaboration_protocol::DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "target is not running; direct message held".to_owned(),
                },
                reachability: None,
                client: None,
            },
        )
        .await
        .unwrap();
    *fixture.presence.0.lock().unwrap() = collaboration_contracts::TargetPresence::Wakeable;
    let first_runtime = fixture.runtime_without_board().await;
    bounded_stage(
        "observe previously held Auto before corruption",
        first_runtime.observe(|event| matches!(event, OwnerObservation::DirectMessageHeld)),
    )
    .await;
    assert_eq!(
        stored_push(&fixture, &held_push_id).await.delivery_state,
        PushDeliveryState::Held
    );
    first_runtime.close().await;

    let mut corrupt_target = target.clone();
    corrupt_target.session_id = SessionId::try_from("corrupt-recovery-target".to_owned()).unwrap();
    let corrupt_draft =
        direct_message_draft(&corrupt_target, "invalid target row", fixture.clock.now());
    let corrupt_push_id = corrupt_draft.push_id.clone();
    fixture
        .interactions
        .insert_push_record(corrupt_draft)
        .await
        .unwrap();
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE router_pushes SET target_session_id='' WHERE push_id=?")
        .bind(corrupt_push_id.as_str())
        .execute(&mut observer)
        .await
        .unwrap();

    let recovery_targets = fixture
        .interactions
        .direct_message_recovery_targets()
        .await
        .unwrap();
    assert_eq!(recovery_targets, vec![target.clone()]);

    *fixture.presence.0.lock().unwrap() = collaboration_contracts::TargetPresence::Running;
    let recovered_runtime = fixture.runtime_without_board().await;
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        recovered_runtime
            .requests
            .lock()
            .await
            .recv()
            .await
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.target, target);
    assert_eq!(request.payload.push_id, held_push_id);
    assert!(
        request
            .payload
            .line
            .as_str()
            .contains("held DM survives restart")
    );
    recovered_runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        recovered_runtime.observe(|event| matches!(event, OwnerObservation::DirectMessageSettled)),
    )
    .await
    .expect("restarted held DM should settle");
    assert_eq!(
        stored_push(&fixture, &held_push_id).await.delivery_state,
        PushDeliveryState::Delivered
    );
    let invalid_target_session: String =
        sqlx::query_scalar("SELECT target_session_id FROM router_pushes WHERE push_id=?")
            .bind(corrupt_push_id.as_str())
            .fetch_one(&mut observer)
            .await
            .unwrap();
    assert_eq!(invalid_target_session, "", "corrupt row stays unchanged");

    observer.close().await.unwrap();
    recovered_runtime.close().await;
}
