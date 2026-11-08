//! Notice debt, receiving append and feed cursors share the actual Fleet owner.
use super::composed_fleet_fixture::*;
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::sync::atomic::Ordering;

fn request(create: bool) -> TestResult<FleetRequest> {
    let name = if create {
        "single Fleet notice"
    } else {
        "single Fleet notice updated"
    }
    .to_owned()
    .try_into()?;
    let description = "debt and receiving append share their owner"
        .to_owned()
        .try_into()?;
    Ok(FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace: "019f0000-0000-7000-8000-000000008181"
            .to_owned()
            .try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "notice owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: if create {
            FleetOperation::WorkspaceCreate { name, description }
        } else {
            FleetOperation::WorkspaceUpdate { name, description }
        },
    })
}
async fn decide(fixture: &Fixture, request: &FleetRequest) -> TestResult<FleetRequestResult> {
    let answer = fixture.call("session", "central.fleet-metadata", Value::Null,
        json!({"origin":{"installation":SERVICE_A,"machine":"notice fixture"},"request":request})).await?;
    let answer: FleetRequestResult = serde_json::from_value(answer)?;
    if !matches!(
        &answer,
        FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ) {
        return Err(format!("notice mutation not accepted: {answer:?}").into());
    }
    Ok(answer)
}
async fn feed(fixture: &Fixture, after: u64) -> TestResult<BoardReadPage> {
    Ok(serde_json::from_value(
        fixture
            .call(
                "session",
                "central.fleet-read",
                Value::Null,
                json!({"kind":"feed","value":{"after":after}}),
            )
            .await?,
    )?)
}
async fn notices(fixture: &Fixture) -> TestResult<Vec<(String, i64)>> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        query::<sqlx_turso::Turso>("SELECT machine_id,notice_id,position,kind FROM machine_control_project_notices ORDER BY machine_id,position")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter()
            .map(|row| {
                if row.try_get::<String,_>("machine_id").map_err(StoreError::backend)? != A || row.try_get::<String,_>("kind").map_err(StoreError::backend)? != "project_list_changed" {
                    return Err(StoreError::Aborted {reason:"unexpected notice recipient or kind".into()});
                }
                Ok((row.try_get("notice_id").map_err(StoreError::backend)?,row.try_get("position").map_err(StoreError::backend)?))
            }).collect::<Result<_,StoreError>>()
    })).await?)
}
fn calls(fixture: &Fixture) -> TestResult<Vec<Value>> {
    Ok(fixture
        .io
        .notice_calls
        .lock()
        .map_err(|_| "notice call log poisoned")?
        .clone())
}
async fn reader(fixture: &Fixture) -> TestResult<(String, i64)> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT reader_id,last_read_position FROM fleet_feed_readers WHERE installation=?").bind(SERVICE_A).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("reader_id").map_err(StoreError::backend)?,row.try_get("last_read_position").map_err(StoreError::backend)?))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertions observe persisted debt and an idempotent append across the same Fleet store reopening."
)]
#[tokio::test]
async fn rejected_local_dispatch_recovers_after_fleet_reopen_without_changing_the_notice_call()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    feed(&fixture, 0).await?;
    let create = request(true)?;
    let answer = decide(&fixture, &create).await?;
    assert!(
        calls(&fixture)?.is_empty(),
        "inbound mutation dispatched notice work"
    );
    assert!(notices(&fixture).await?.is_empty());
    fixture.io.reject_next_notice.store(true, Ordering::SeqCst);
    assert_eq!(
        fixture
            .runtime
            .conformance_drive_operations()
            .await?
            .retried
            .len(),
        1
    );
    let frozen = calls(&fixture)?;
    assert_eq!(frozen.len(), 1);
    assert_eq!(
        frozen[0]["request"]["kind"], "projectListChanged",
        "directory changes must produce the shared project-list hint"
    );
    assert!(notices(&fixture).await?.is_empty());
    fixture.close().await?;

    let reopened = Fixture::open(root.path(), NOW + 10_000).await?;
    assert_eq!(
        reopened
            .runtime
            .conformance_drive_operations()
            .await?
            .retried
            .len(),
        1
    );
    assert_eq!(calls(&reopened)?, frozen);
    assert!(notices(&reopened).await?.is_empty());
    assert_eq!(
        reopened
            .runtime
            .conformance_drive_operations()
            .await?
            .completed
            .len(),
        1
    );
    let committed = notices(&reopened).await?;
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].1, 1);
    assert!(
        matches!(feed(&reopened, 0).await?, BoardReadPage::FleetFeed(FleetFeedRead::Page { through, head, records, .. }) if through.get() == 2 && head.get() == 2 && records.len() == 2)
    );
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &committed[0].0
    )?));
    reopened.io.advance(10_000);
    assert_eq!(
        reopened
            .runtime
            .conformance_drive_operations()
            .await?
            .completed
            .len(),
        1
    );
    assert_eq!(
        calls(&reopened)?,
        vec![frozen[0].clone(), frozen[0].clone()]
    );
    assert_eq!(notices(&reopened).await?, committed);
    assert_eq!(decide(&reopened, &create).await?, answer);
    assert!(
        reopened
            .runtime
            .conformance_drive_operations()
            .await?
            .completed
            .is_empty()
    );
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real post-callback uniqueness failure must roll back mutation, notice debt and wake intent together."
)]
#[tokio::test]
async fn post_callback_sql_failure_rolls_back_metadata_feed_notice_debt_and_wake_intent()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    feed(&fixture, 0).await?;
    let before = fixture.io.wakes()?;
    fixture.faults.armed.store(true, Ordering::SeqCst);
    let create = request(true)?;
    assert!(decide(&fixture, &create).await.is_err());
    assert!(fixture.faults.injected.load(Ordering::SeqCst));
    let counts=fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM fleet_workspaces) AS workspaces,(SELECT count(*) FROM fleet_feed) AS feed,(SELECT count(*) FROM _fleet_sync_operations WHERE kind='central.fleet-notice') AS notices").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get::<i64,_>("workspaces").map_err(StoreError::backend)?,row.try_get::<i64,_>("feed").map_err(StoreError::backend)?,row.try_get::<i64,_>("notices").map_err(StoreError::backend)?))
    })).await?;
    assert_eq!(counts, (0, 0, 0));
    assert_eq!(fixture.io.wakes()?, before);
    assert!(calls(&fixture)?.is_empty());
    assert!(notices(&fixture).await?.is_empty());
    decide(&fixture, &create).await?;
    assert_eq!(
        fixture
            .runtime
            .conformance_drive_operations()
            .await?
            .retried
            .len(),
        1
    );
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    fixture.io.advance(10_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert_eq!(notices(&fixture).await?.len(), 1);
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A lost reply after the receiving append commits must replay the same call and preserve exactly one row."
)]
#[tokio::test]
async fn lost_local_receipt_after_append_survives_fleet_reopen_without_a_second_notice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    feed(&fixture, 0).await?;
    decide(&fixture, &request(true)?).await?;
    assert_eq!(
        fixture
            .runtime
            .conformance_drive_operations()
            .await?
            .retried
            .len(),
        1
    );
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let committed = notices(&fixture).await?;
    assert_eq!(committed.len(), 1);
    fixture
        .io
        .lose_next_notice_reply
        .store(true, Ordering::SeqCst);
    fixture.io.advance(10_000);
    assert_eq!(
        fixture
            .runtime
            .conformance_drive_operations()
            .await?
            .retried
            .len(),
        1
    );
    assert!(!fixture.io.lose_next_notice_reply.load(Ordering::SeqCst));
    let frozen = calls(&fixture)?;
    assert_eq!(frozen.len(), 2);
    assert_eq!(frozen[0], frozen[1]);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 20_000).await?;
    assert_eq!(
        reopened
            .runtime
            .conformance_drive_operations()
            .await?
            .completed
            .len(),
        1
    );
    assert_eq!(calls(&reopened)?, vec![frozen[0].clone()]);
    assert_eq!(notices(&reopened).await?, committed);
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Offered pages do not advance the applied cursor or suppress future hints; stale applied cursors never regress it."
)]
#[tokio::test]
async fn lost_offered_page_keeps_its_applied_cursor_and_a_later_append_still_hints() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    decide(&fixture, &request(true)?).await?;
    assert!(
        matches!(feed(&fixture,0).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {through,head,..}) if through.get()==2&&head.get()==2)
    );
    let before = reader(&fixture).await?;
    assert_eq!(before.1, 0);
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &before.0
    )?));
    decide(&fixture, &request(false)?).await?;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    fixture.io.advance(10_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let rows = notices(&fixture).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 1);
    assert!(
        matches!(feed(&fixture,4).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,head,through,..}) if records.is_empty() && head.get() == 4 && through.get() == 4)
    );
    feed(&fixture, 2).await?;
    assert_eq!(reader(&fixture).await?, (before.0, 4));
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Each alarm's real SQL must match independently observed dispatches and committed notice appends, including a duplicate-lookup negative control."
)]
#[tokio::test]
async fn each_fleet_alarm_pays_eight_sql_per_append_and_one_lookup_per_dispatched_task()
-> TestResult {
    use super::{
        fleet_alarm_sql::{AlarmSql, validate_alarm},
        fleet_mail_dispatch::Socket,
    };
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let _sender = Socket::open(&fixture, A, SERVICE_A).await?;
    feed(&fixture, 0).await?;
    decide(&fixture, &request(true)?).await?;
    // The receiving task is admitted by the first real dispatch. A later alarm
    // appends; the final replay answers the original sender without appending.
    for (index, expected) in [(0, (1, 0)), (1, (0, 1)), (2, (1, 0)), (3, (0, 0))] {
        if index == 2 {
            fixture.io.advance(10_000);
        }
        let before = (calls(&fixture)?.len(), notices(&fixture).await?.len());
        let trace = AlarmSql::default();
        let report = trace
            .observe(fixture.runtime.conformance_drive_operations())
            .await?;
        assert!(report.failed.is_empty(), "{report:?}");
        let observed = (
            calls(&fixture)?.len() - before.0,
            notices(&fixture).await?.len() - before.1,
        );
        assert_eq!(
            observed, expected,
            "alarm {index} must execute its actual work"
        );
        let statements = trace.statements()?;
        validate_alarm(&statements, observed.0, observed.1)?;
        if observed.0 > 0 {
            let mut duplicate_lookup = statements;
            duplicate_lookup.push(
                include_str!("../../../collaboration-central-core/sql/read_enrollments.sql")
                    .to_owned(),
            );
            assert!(
                validate_alarm(&duplicate_lookup, observed.0, observed.1).is_err(),
                "a second lookup for the same actual task escaped its per-callback cost bound"
            );
        }
    }
    fixture.close().await
}
