//! Real project/native-message interaction for C5's derived scan position.

use super::*;
use crate::subscription_delivery::tests::owner_fixture::{OwnerFixture, human};
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use std::collections::BTreeSet;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
#[path = "intent_read_cost_counter.rs"]
mod intent_read_cost_counter;
use intent_read_cost_counter::{IntentReadCost, IntentReadCounter};
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
    let RequestResult::Decided(RequestDecision::Accepted { effects, .. }) = reply.result else {
        panic!("post accepted");
    };
    let key = effects
        .entries()
        .iter()
        .map(|(reference, _)| reference)
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
    let cursor = record.envelope().position.project_cursor().unwrap();
    read.finish().await.unwrap();
    cursor
}
async fn measured_pass(
    service: &crate::NotificationPublisher,
    fixture: &OwnerFixture,
) -> IntentReadCost {
    let counter = IntentReadCounter::default();
    let subscriber = tracing_subscriber::registry().with(counter.clone());
    Box::pin(
        service
            .consume_project_intents(&fixture.project)
            .with_subscriber(subscriber),
    )
    .await
    .unwrap();
    counter.cost()
}
async fn deferred_history_cost(size: usize, counter: &IntentReadCounter) -> IntentReadCost {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime_without_board().await;
    for _ in 0..size {
        addressed_post(&fixture, human("attention")).await;
    }
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let new_intent = addressed_post(&fixture, human("attention")).await;
    let before = counter.cost();
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let cost = counter.cost().since(before);
    assert!(
        fixture
            .interactions
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
    let small = Box::pin(deferred_history_cost(8, &counter)).await;
    let large = Box::pin(deferred_history_cost(180, &counter)).await;
    eprintln!("intent pass costs: small={small:?}, large={large:?}");
    assert!(
        small.statements > 0 && small.records > 0,
        "counter observed no real work"
    );
    assert_eq!(small, large, "historical Deferred intents were rescanned");
}

#[tokio::test]
async fn restart_skips_consumed_history_even_when_its_body_is_corrupt() {
    let mut fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime_without_board().await;
    let old_intent = addressed_post(&fixture, human("attention")).await;
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
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
    assert_eq!(
        sqlx::query("UPDATE notify_intents SET state = json_quote('corrupt') WHERE intent_id = ?")
            .bind(old_intent.as_str())
            .execute(&mut connection)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    connection.close().await.unwrap();
    fixture = fixture.reopen_messages().await;
    let restarted = fixture.runtime_without_board().await;
    let cost = Box::pin(measured_pass(&restarted.publisher, &fixture)).await;
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

use crate::native_worker_faults as native_intent_faults;

async fn native_sql(fixture: &OwnerFixture, statement: &'static str) {
    native_intent_faults::execute(fixture.interactions.clone(), statement, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn deleting_the_derived_cursor_preserves_exactly_once_consumption() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime_without_board().await;
    for _ in 0..3 {
        addressed_post(&fixture, human("attention")).await;
    }
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    native_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert!(
        fixture
            .interactions
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
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    // Another publisher pass can already own delivery after custody commits.
    // Observe both real requests before checking their identities.
    let delivered = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut requests = runtime.requests.lock().await;
        let first = requests.recv().await.unwrap();
        let second = requests.recv().await.unwrap();
        assert_ne!(first.payload.push_id, second.payload.push_id);
        vec![first, second]
    })
    .await
    .expect("both addressed intent requests must reach the native client");
    let mut delivered_intents = BTreeSet::new();
    for request in delivered {
        let push = fixture
            .interactions
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
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "cursor recovery repeated a delivery"
    );
    // Revisit the already delivered local versions, so the consumed-version probe
    // rather than the saved scan cursor must suppress both decode and delivery.
    native_sql(&fixture, "DELETE FROM board_intent_scan_cursors").await;
    assert_eq!(
        Box::pin(measured_pass(&runtime.publisher, &fixture))
            .await
            .records,
        0
    );
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "old delivered version repeated after cursor loss"
    );
    assert!(
        fixture
            .interactions
            .intents_scanned_through(&fixture.project)
            .await
            .unwrap()
            .is_some()
    );
    runtime.close().await;
}

#[tokio::test]
async fn failed_push_preparation_holds_the_contiguous_prefix_and_retries_the_claim() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime_without_board().await;
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
    let first_position = first_record.envelope().position.project_cursor().unwrap();
    read.finish().await.unwrap();
    native_sql(&fixture, "CREATE TRIGGER block_intent_push BEFORE INSERT ON router_pushes BEGIN SELECT RAISE(FAIL, 'injected push preparation failure'); END").await;
    assert!(
        runtime
            .publisher
            .consume_project_intents(&fixture.project)
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .interactions
            .intents_scanned_through(&fixture.project)
            .await
            .unwrap(),
        Some(first_position)
    );
    assert!(
        fixture
            .interactions
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
            .interactions
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
    assert!(matches!(intent.state(), IntentState::HandedOff { .. }));
    read.finish().await.unwrap();
    native_sql(&fixture, "DROP TRIGGER block_intent_push").await;
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(runtime.requests.lock().await.try_recv().is_ok());
    assert!(
        fixture
            .interactions
            .intent_consumed(
                &fixture.project,
                &second,
                current_intent_cursor(&fixture, &second).await
            )
            .await
            .unwrap()
    );
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
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
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime_without_board().await;
    let intent_id = addressed_post(&fixture, fixture.reader.clone()).await;
    let service = runtime.publisher.clone();
    let project = fixture.project.clone();
    let pass = tokio::spawn(async move { service.consume_project_intents(&project).await });
    let request = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.requests.lock().await.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    let expired = crate::test_project_runtime::expire_lease(&fixture.registry, &fixture.project)
        .await
        .unwrap();
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Rejected(
            collaboration_protocol::DeliveryRejection {
                native_failure: None,
                reason: collaboration_protocol::DeliveryRejectionReason::PermissionDenied,
                next_action: collaboration_protocol::DeliveryNextAction::CorrectRequest,
                client_code: None,
                detail: Some("definitive endpoint refusal".to_owned()),
                claims: None,
            },
        ))
        .unwrap();
    pass.await.unwrap().unwrap();
    let completed = fixture
        .await_push_state(
            &request.payload.push_id,
            collaboration_protocol::PushDeliveryState::Rejected,
        )
        .await;
    let item = fleet_sync_protocol::ItemId::new(
        uuid::Uuid::parse_str(request.payload.push_id.as_str()).unwrap(),
    )
    .unwrap();
    let terminal = runtime.local_mail_outcome(item).await;
    assert!(matches!(
        &terminal,
        Some(fleet_sync_protocol::RecipientOutcome::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::RecipientRefused { code }
        }) if code.as_str() == "permissionDenied"
    ));
    assert!(matches!(
        &completed.last_outcome.as_ref().unwrap().outcome,
        collaboration_protocol::DeliveryOutcome::Rejected(rejection)
            if rejection.detail.as_deref() == Some("definitive endpoint refusal")
    ));
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let Some(Record::NotifyIntent(handed_off)) =
        read.record(&intent_id.clone().into()).await.unwrap()
    else {
        panic!("intent");
    };
    assert!(matches!(handed_off.state(), IntentState::HandedOff { item }
        if item.as_str() == request.payload.push_id.as_str()));
    let handoff = read
        .mail_handoff(
            &request
                .payload
                .push_id
                .as_str()
                .to_owned()
                .try_into()
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        read.mail_settled_through().await.unwrap(),
        HandoffSeq::ZERO,
        "frontier advanced while its project writer lease was expired"
    );
    read.finish().await.unwrap();

    // Custody/outcome settlement is independent of the project writer. The
    // replicated frontier still requires its live lease at the owning admission.
    let expired_frontier = runtime
        .publisher
        .advance_project_frontier_for_test(&fixture.project)
        .await;
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(
        read.mail_settled_through().await.unwrap(),
        HandoffSeq::ZERO,
        "frontier advanced while its project writer lease was expired"
    );
    read.finish().await.unwrap();
    assert!(expired_frontier.is_err());
    assert_eq!(runtime.local_mail_outcome(item).await, terminal);
    crate::test_project_runtime::renew_lease(&fixture.registry, &fixture.project, expired)
        .await
        .unwrap();
    runtime
        .publisher
        .advance_project_frontier_for_test(&fixture.project)
        .await
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(read.mail_settled_through().await.unwrap(), handoff.sequence);
    assert_eq!(
        read.record(&intent_id.clone().into()).await.unwrap(),
        Some(Record::NotifyIntent(handed_off))
    );
    read.finish().await.unwrap();
    assert_eq!(runtime.local_mail_outcome(item).await, terminal);
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        fixture
            .interactions
            .intent_consumed(
                &fixture.project,
                &intent_id,
                current_intent_cursor(&fixture, &intent_id).await,
            )
            .await
            .unwrap()
    );
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "lease renewal repeated the refused client effect"
    );
    runtime.close().await;
}
