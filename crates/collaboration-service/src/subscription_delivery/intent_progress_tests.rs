//! Real project/automation interaction for C5's derived scan position.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
use super::*;
use crate::service_identity::subscription_delivery::tests::owner_fixture::{OwnerFixture, human};
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
#[path = "intent_read_cost_counter.rs"]
mod intent_read_cost_counter;
use intent_read_cost_counter::{IntentReadCost, IntentReadCounter};
#[path = "intent_epoch_floor_tests.rs"]
mod intent_epoch_floor_tests;
#[path = "intent_reinstatement_recovery_tests.rs"]
mod intent_reinstatement_recovery_tests;
#[path = "intent_role_delivery_tests.rs"]
mod intent_role_delivery_tests;

async fn addressed_post(fixture: &OwnerFixture, recipient: Identity) -> NotifyIntentId {
    let operation = serde_json::from_value(json!({
        "kind":"threadPost","postId":PostId::generate(),"threadId":fixture.root,
        "text":"addressed work","references":[],"addressees":[{"kind":"actor","recipient":recipient}]
    })).unwrap();
    let reply = fixture
        .write_as(human("writer"), operation, fixture.clock.now())
        .await;
    let RequestResult::Decided(RequestDecision::Accepted { records, .. }) = reply.result else {
        panic!("post accepted");
    };
    let key = records
        .into_iter()
        .find(|key| key.kind == RecordKind::NotifyIntent)
        .unwrap();
    NotifyIntentId::try_from(key.id.as_str().to_owned()).unwrap()
}
async fn current_intent_cursor(fixture: &OwnerFixture, intent: &NotifyIntentId) -> FeedCursor {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let record = read.record(&intent.clone().into()).await.unwrap().unwrap();
    let RecordPosition::Project(cursor) = record.envelope().position else {
        panic!("project intent cursor");
    };
    read.finish().await.unwrap();
    cursor
}
async fn measured_pass(
    service: &SubscriptionDeliveryService,
    fixture: &OwnerFixture,
) -> IntentReadCost {
    let counter = IntentReadCounter::default();
    let subscriber = tracing_subscriber::registry().with(counter.clone());
    service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .with_subscriber(subscriber)
        .await
        .unwrap();
    counter.cost()
}
async fn deferred_history_cost(size: usize, counter: &IntentReadCounter) -> IntentReadCost {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    for _ in 0..size {
        addressed_post(&fixture, human("attention")).await;
    }
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    let new_intent = addressed_post(&fixture, human("attention")).await;
    let before = counter.cost();
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    let cost = counter.cost().since(before);
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &new_intent,
                current_intent_cursor(&fixture, &new_intent).await
            )
            .await
            .unwrap()
    );
    runtime.close().await;
    cost
}
#[tokio::test]
async fn deferred_history_does_not_grow_the_next_intent_pass() {
    const CHILD_ENV: &str = "CODEX_ROUTER_INTENT_COST_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let test = concat!(
            module_path!(),
            "::deferred_history_does_not_grow_the_next_intent_pass"
        );
        let status = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test.split_once("::").unwrap().1, "--nocapture"])
            .env(CHILD_ENV, "1")
            .status()
            .await
            .unwrap();
        assert!(status.success(), "isolated intent-cost proof failed");
        return;
    }
    let counter = IntentReadCounter::default();
    counter.install_global();
    let small = deferred_history_cost(8, &counter).await;
    let large = deferred_history_cost(180, &counter).await;
    eprintln!("intent pass costs: small={small:?}, large={large:?}");
    assert!(
        small.statements > 0 && small.records > 0,
        "counter observed no real work"
    );
    assert_eq!(small, large, "historical Deferred intents were rescanned");
}

#[tokio::test]
async fn restart_skips_consumed_history_even_when_its_body_is_corrupt() {
    let mut fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    let old_intent = addressed_post(&fixture, human("attention")).await;
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    runtime.close().await;
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
    sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.state', 'corrupt') WHERE kind = 'notifyIntent' AND id = ?")
        .bind(old_intent.as_str()).execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    fixture.push_store = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(
            &fixture.directory.path().join("automation.sqlite"),
        )
        .await
        .unwrap(),
    ));
    let restarted = fixture.runtime_without_board().await;
    let cost = measured_pass(&restarted.service, &fixture).await;
    assert_eq!(
        cost.records, 0,
        "restart decoded history behind the saved cursor"
    );
    let mut read = store.snapshot().await.unwrap();
    assert!(
        read.record(&old_intent.into()).await.is_err(),
        "direct body inspection must still reject corruption"
    );
    read.finish().await.unwrap();
    restarted.close().await;
}

async fn automation_sql(fixture: &OwnerFixture, statement: &'static str) {
    let mut connection = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(fixture.directory.path().join("automation.sqlite"))
        .connect()
        .await
        .unwrap();
    sqlx::query(statement)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn deleting_the_derived_cursor_preserves_exactly_once_consumption() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    for _ in 0..3 {
        addressed_post(&fixture, human("attention")).await;
    }
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intents_scanned_through(&fixture.project)
            .await
            .unwrap()
            .is_none()
    );
    let first = addressed_post(&fixture, fixture.reader.clone()).await;
    let second = addressed_post(&fixture, fixture.reader.clone()).await;
    for _ in 0..2 {
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::Started)
            .unwrap();
    }
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    let delivered = {
        let mut requests = runtime.requests.lock().await;
        let first = requests.try_recv().unwrap();
        let second = requests.try_recv().unwrap();
        assert_ne!(first.payload.push_id, second.payload.push_id);
        vec![first, second]
    };
    let mut delivered_intents = BTreeSet::new();
    for request in delivered {
        let push = fixture
            .push_store
            .lock()
            .await
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        let collaboration_protocol::PushHeaderFacts::BoardNotification { intent, .. } =
            push.header_facts
        else {
            panic!("board notification");
        };
        delivered_intents.insert(intent);
    }
    assert_eq!(delivered_intents, BTreeSet::from([first, second]));
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "cursor recovery repeated a delivery"
    );
    // Revisit the already delivered local versions, so the consumed-version probe
    // rather than the saved scan cursor must suppress both decode and delivery.
    automation_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert_eq!(measured_pass(&runtime.service, &fixture).await.records, 0);
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "old delivered version repeated after cursor loss"
    );
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intents_scanned_through(&fixture.project)
            .await
            .unwrap()
            .is_some()
    );
    runtime.close().await;
}

#[tokio::test]
async fn failed_push_preparation_holds_the_contiguous_prefix_and_retries_the_claim() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    let first = addressed_post(&fixture, human("attention")).await;
    let second = addressed_post(&fixture, fixture.reader.clone()).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let first_record = read.record(&first.clone().into()).await.unwrap().unwrap();
    let RecordPosition::Project(first_position) = first_record.envelope().position else {
        panic!("project position");
    };
    read.finish().await.unwrap();
    automation_sql(&fixture, "CREATE TRIGGER block_intent_push BEFORE INSERT ON router_pushes BEGIN SELECT RAISE(FAIL, 'injected push preparation failure'); END").await;
    assert!(
        runtime
            .service
            .consume_project_intents(&fixture.admission, &fixture.project)
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .push_store
            .lock()
            .await
            .intents_scanned_through(&fixture.project)
            .await
            .unwrap(),
        Some(first_position)
    );
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &first,
                current_intent_cursor(&fixture, &first).await
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
                &second,
                current_intent_cursor(&fixture, &second).await
            )
            .await
            .unwrap()
    );
    let mut read = store.snapshot().await.unwrap();
    let Some(Record::NotifyIntent(intent)) = read.record(&second.clone().into()).await.unwrap()
    else {
        panic!("intent");
    };
    assert!(matches!(intent.state(), IntentState::HandedOff {}));
    read.finish().await.unwrap();
    automation_sql(&fixture, "DROP TRIGGER block_intent_push").await;
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_ok());
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &second,
                current_intent_cursor(&fixture, &second).await
            )
            .await
            .unwrap()
    );
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "retry repeated the client effect"
    );
    runtime.close().await;
}

#[tokio::test]
async fn a_recorded_push_refusal_is_settled_after_a_transient_lease_failure() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime_without_board().await;
    let intent_id = addressed_post(&fixture, fixture.reader.clone()).await;
    let service = runtime.service.clone();
    let admission = fixture.admission.clone();
    let project = fixture.project.clone();
    let pass =
        tokio::spawn(async move { service.consume_project_intents(&admission, &project).await });
    let request = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.requests.lock().await.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    fixture
        .admission
        .leases
        .set_grant(
            fixture.project.clone(),
            crate::GrantResult::NotHolder {
                current: Some(EpochNumber::try_from(1).unwrap()),
            },
        )
        .await;
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Rejected(
            collaboration_protocol::DeliveryRejection {
                reason: collaboration_protocol::DeliveryRejectionReason::PermissionDenied,
                next_action: collaboration_protocol::DeliveryNextAction::CorrectRequest,
                client_code: None,
                detail: Some("definitive endpoint refusal".to_owned()),
                claims: None,
            },
        ))
        .unwrap();
    assert!(pass.await.unwrap().is_err());
    assert!(
        !fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &intent_id,
                current_intent_cursor(&fixture, &intent_id).await
            )
            .await
            .unwrap()
    );
    let completed = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        completed.delivery_state,
        collaboration_protocol::PushDeliveryState::Rejected
    );
    fixture
        .admission
        .leases
        .set_grant(
            fixture.project.clone(),
            crate::GrantResult::Holder {
                epoch: EpochNumber::try_from(1).unwrap(),
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(3600),
            },
        )
        .await;
    runtime
        .service
        .consume_project_intents(&fixture.admission, &fixture.project)
        .await
        .unwrap();
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let Some(Record::NotifyIntent(intent)) = read.record(&intent_id.clone().into()).await.unwrap()
    else {
        panic!("intent");
    };
    assert!(
        matches!(intent.state(), IntentState::Refused { reason: IntentRefusal::Delivery { detail } } if detail == "definitive endpoint refusal")
    );
    read.finish().await.unwrap();
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .intent_consumed(
                &fixture.project,
                &intent_id,
                current_intent_cursor(&fixture, &intent_id).await
            )
            .await
            .unwrap()
    );
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "retry repeated the refused client effect"
    );
    runtime.close().await;
}
