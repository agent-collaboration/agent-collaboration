//! A mutation appends its notices in its own transaction on the actual Fleet owner; feed cursors live beside them.
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
/// Workspaces, feed entries, notice rows and notice engine jobs.
async fn ledger_counts(fixture: &Fixture) -> TestResult<(i64, i64, i64, i64)> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM fleet_workspaces) AS workspaces,(SELECT count(*) FROM fleet_feed) AS feed,(SELECT count(*) FROM machine_control_project_notices) AS notices,(SELECT count(*) FROM _fleet_sync_operations WHERE kind='central.fleet-notice') AS jobs").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get::<i64,_>("workspaces").map_err(StoreError::backend)?,row.try_get::<i64,_>("feed").map_err(StoreError::backend)?,row.try_get::<i64,_>("notices").map_err(StoreError::backend)?,row.try_get::<i64,_>("jobs").map_err(StoreError::backend)?))
    })).await?)
}
async fn reader(fixture: &Fixture) -> TestResult<(String, i64)> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT reader_id,last_read_position FROM fleet_feed_readers WHERE installation=?").bind(SERVICE_A).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("reader_id").map_err(StoreError::backend)?,row.try_get("last_read_position").map_err(StoreError::backend)?))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertions observe one appended notice across a Fleet reopen and an idempotent replay."
)]
#[tokio::test]
async fn mutation_appends_one_notice_that_survives_reopen_and_a_replayed_request_adds_none()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    feed(&fixture, 0).await?;
    let create = request(true)?;
    let answer = decide(&fixture, &create).await?;
    // Slice 4a: the notice is appended in the mutation's own transaction, so
    // it exists before any engine drive and no object call carries it.
    let committed = notices(&fixture).await?;
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].1, 1);
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &committed[0].0
    )?));
    let drive = fixture.runtime.conformance_drive_operations().await?;
    assert!(
        drive.retried.is_empty() && drive.completed.is_empty(),
        "{drive:?}"
    );
    assert!(
        calls(&fixture)?.is_empty(),
        "a notice went through an object call"
    );
    fixture.close().await?;

    let reopened = Fixture::open(root.path(), NOW + 10_000).await?;
    assert_eq!(notices(&reopened).await?, committed);
    assert!(
        matches!(feed(&reopened, 0).await?, BoardReadPage::FleetFeed(FleetFeedRead::Page { through, head, records, .. }) if through.get() == 2 && head.get() == 2 && records.len() == 2)
    );
    // A replayed request answers from its recorded decision and appends nothing.
    assert_eq!(decide(&reopened, &create).await?, answer);
    assert_eq!(notices(&reopened).await?, committed);
    assert!(calls(&reopened)?.is_empty());
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real post-callback uniqueness failure must roll back the mutation and its notice together; a successful one appends its notice in the same transaction."
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
    // The failed mutation rolled back its records, feed entry and notice together.
    assert_eq!(ledger_counts(&fixture).await?, (0, 0, 0, 0));
    assert_eq!(fixture.io.wakes()?, before);
    assert!(calls(&fixture)?.is_empty());
    decide(&fixture, &create).await?;
    // Slice 4a: the notice is appended in the mutation's own transaction. No
    // notice job is admitted and no object call carries it.
    let (workspaces, feed_entries, notice_rows, jobs) = ledger_counts(&fixture).await?;
    assert_eq!((workspaces, notice_rows, jobs), (1, 1, 0));
    assert!(feed_entries > 0);
    assert!(calls(&fixture)?.is_empty());
    assert_eq!(notices(&fixture).await?.len(), 1);
    fixture.close().await
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
    reason = "Engine drives after a mutation must do no notice work: the mutation appended it, with a lookup negative control."
)]
#[tokio::test]
async fn engine_drives_after_a_mutation_do_no_notice_work() -> TestResult {
    use super::{
        fleet_alarm_sql::{AlarmSql, validate_alarm},
        fleet_mail_dispatch::Socket,
    };
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let _sender = Socket::open(&fixture, A, SERVICE_A).await?;
    feed(&fixture, 0).await?;
    decide(&fixture, &request(true)?).await?;
    let appended = notices(&fixture).await?;
    assert_eq!(appended.len(), 1, "the mutation did not append its notice");
    for index in 0..3 {
        if index == 1 {
            fixture.io.advance(10_000);
        }
        let trace = AlarmSql::default();
        let report = trace
            .observe(fixture.runtime.conformance_drive_operations())
            .await?;
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(
            calls(&fixture)?.is_empty(),
            "drive {index} dispatched notice work"
        );
        assert_eq!(notices(&fixture).await?, appended, "drive {index} appended");
        let statements = trace.statements()?;
        validate_alarm(&statements, 0, 0)?;
        let mut stray_lookup = statements;
        stray_lookup.push(
            include_str!("../../../collaboration-central-core/sql/read_enrollments.sql").to_owned(),
        );
        assert!(
            validate_alarm(&stray_lookup, 0, 0).is_err(),
            "a recipient lookup escaped the drive's notice-free bound"
        );
    }
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertion inspects the idle event's real SQL while fixture errors propagate."
)]
#[tokio::test]
async fn an_idle_event_reads_no_machine_notice_progress() -> TestResult {
    use super::fleet_alarm_sql::AlarmSql;
    const PROGRESS: &str = include_str!(
        "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_applied.sql"
    );
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    feed(&fixture, 0).await?;
    decide(&fixture, &request(true)?).await?;
    assert_eq!(notices(&fixture).await?.len(), 1);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    // The post-event push reads only machines the event appended to; this
    // alarm appended nothing, so no machine's progress is read.
    let trace = AlarmSql::default();
    trace
        .observe(crate::kernel_alarm::deliver_alarm(&fixture.runtime))
        .await?;
    let progress_reads = trace
        .statements()?
        .into_iter()
        .filter(|statement| statement.trim() == PROGRESS.trim())
        .count();
    assert_eq!(progress_reads, 0, "an idle event read a notice stream");
    fixture.close().await
}
