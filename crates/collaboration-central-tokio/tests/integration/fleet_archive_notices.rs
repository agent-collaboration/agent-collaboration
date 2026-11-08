//! Two projects share one verified holder: archive notices and feed hints append once.
//! Existing grants and final project reports are component inputs; no Copy execution is claimed.
use super::composed_fleet_fixture::*;
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{ArchiveReportOutcome, FleetOperation, FleetRequest, FleetRequestResult};
use runtime_primitives::Clock;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

const SECOND: &str = "019f0000-0000-7000-8000-000000008198";
const BOARD: &str = "019f0000-0000-7000-8000-000000008182";

async fn prepare(fixture: &Fixture) -> TestResult {
    fixture.arrange_directory().await?;
    let created = fixture
        .metadata(FleetOperation::ProjectDirectoryCreate {
            project: SECOND.to_owned().try_into()?,
            board: BOARD.to_owned().try_into()?,
            title: "second shared-holder project".to_owned().try_into()?,
        })
        .await?;
    require_accepted(created)?;
    fixture.store.transaction(|scope|Box::pin(async move {
        for project in [PROJECT,SECOND] {
            // Arrange pre-existing authority only. Actual Renew below executes
            // the production Core hook and verifies its holder through enrollment.
            query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_databases(database,lifecycle_kind,origin_kind,revision_epoch,revision_change,changed_at,created_at,updated_at) VALUES(?,'registered','empty','1','1',?,?,?)")
                .bind(project).bind(NOW).bind(NOW).bind(NOW).execute(scope.executor()).await.map_err(StoreError::backend)?;
            query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_leases(database,holder,epoch,expires_at,last_renewed_at,created_at,updated_at) VALUES(?,?,'1',?,?,?,?)")
                .bind(project).bind(A).bind(NOW+1_000).bind(NOW).bind(NOW).bind(NOW).execute(scope.executor()).await.map_err(StoreError::backend)?;
            query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_epoch_floor(database,epoch,created_at,updated_at) VALUES(?,'1',?,?)")
                .bind(project).bind(NOW).bind(NOW).execute(scope.executor()).await.map_err(StoreError::backend)?;
        }
        Ok(())
    })).await?;
    for project in [PROJECT, SECOND] {
        let answer=fixture.call("session","central.session-lease",Value::Null,json!({
            "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
            "request":{"kind":"renew","value":{"attempt":durable_steps::Uuid::now_v7(),"database":project,"epoch":1}}
        })).await?;
        if !matches!(
            serde_json::from_value::<fleet_protocol::LeaseAnswer>(answer)?,
            fleet_protocol::LeaseAnswer::Renew(fleet_protocol::RenewAnswer::Renewed {
                replayed: false,
                ..
            })
        ) {
            return Err("genuine shared-holder renewal failed".into());
        }
    }
    drain(fixture).await
}
fn require_accepted(answer: Value) -> TestResult {
    if !matches!(
        serde_json::from_value::<FleetRequestResult>(answer)?,
        FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ) {
        return Err("archive setup/decision was not accepted".into());
    }
    Ok(())
}
async fn drain(fixture: &Fixture) -> TestResult {
    for _ in 0..8 {
        let report = fixture.runtime.conformance_drive_operations().await?;
        if !report.failed.is_empty() {
            return Err(format!("notice work failed to settle: {report:?}").into());
        }
        // The caller first admits its real receiving append, then retries to
        // collect that commit. Drive the admitted receiver and use the actual
        // released retry schedule; a retry is not a failed notice or wall sleep.
        let next=fixture.store.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT min(next_attempt_at) AS next FROM _fleet_sync_operations WHERE state='running' AND drive_until IS NULL")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?.try_get::<Option<i64>,_>("next").map_err(StoreError::backend)
        })).await?;
        let Some(next) = next else {
            return Ok(());
        };
        if next > fixture.io.now().unix_millis() {
            fixture.io.advance(next - fixture.io.now().unix_millis());
        }
    }
    Err("notice work did not reach a causal idle boundary".into())
}
#[derive(Debug, PartialEq)]
struct Notice {
    position: i64,
    kind: String,
    project: Option<String>,
    epoch: Option<i64>,
}
async fn notices(fixture: &Fixture) -> TestResult<Vec<Notice>> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let rows=query::<sqlx_turso::Turso>("SELECT machine_id,position,kind,project_id,epoch FROM machine_control_project_notices ORDER BY position")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        rows.into_iter().map(|row| {
            if row.try_get::<String,_>("machine_id").map_err(StoreError::backend)?!=A {return Err(StoreError::backend("archive notified another holder"));}
            Ok(Notice {position:row.try_get("position").map_err(StoreError::backend)?,kind:row.try_get("kind").map_err(StoreError::backend)?,project:row.try_get("project_id").map_err(StoreError::backend)?,epoch:row.try_get("epoch").map_err(StoreError::backend)?})
        }).collect()
    })).await?)
}
async fn head(fixture: &Fixture) -> TestResult<i64> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT last_position FROM fleet_feed_state WHERE name='fleet'",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("last_position")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}
async fn feed(fixture: &Fixture, after: i64) -> TestResult<BoardReadPage> {
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
fn archive() -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace: "019f0000-0000-7000-8000-000000008181"
            .to_owned()
            .try_into()?,
        actor: message_board::RecordedActor::Human {
            human_id: "archive notice owner".to_owned().try_into()?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation: FleetOperation::BoardArchive {
            board: BOARD.to_owned().try_into()?,
        },
    })
}
async fn decide(fixture: &Fixture, request: &FleetRequest) -> TestResult<Value> {
    fixture.call("session","central.fleet-metadata",Value::Null,json!({"origin":{"installation":SERVICE_A,"machine":"shared-holder archive"},"request":request})).await
}
async fn report(fixture: &Fixture, project: &str) -> TestResult<ArchiveReportOutcome> {
    Ok(serde_json::from_value(
        fixture
            .call(
                "session",
                "central.archive-report",
                Value::Null,
                json!({"board":BOARD,"project":project,"position":{"epoch":1,"sequence":0}}),
            )
            .await?,
    )?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Both real hook views must deduplicate their shared holder and final archive feed hint; replays must not add records, positions or notices."
)]
async fn board_archiving_notifies_its_shared_holder_once_and_final_reports_hint_the_feed_once()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    prepare(&fixture).await?;
    assert!(notices(&fixture).await?.is_empty());
    let before = head(&fixture).await?;
    let request = archive()?;
    let answer = decide(&fixture, &request).await?;
    require_accepted(answer.clone())?;
    // The holder's notice is appended in the archiving mutation's own transaction
    // (slice 4a: no engine job), so it is there before any alarm or drain.
    let archiving = Notice {
        position: 1,
        kind: "project_list_changed".into(),
        project: None,
        epoch: None,
    };
    assert_eq!(notices(&fixture).await?, vec![archiving]);
    assert_eq!(head(&fixture).await?, before + 2);
    assert_eq!(decide(&fixture, &request).await?, answer);
    drain(&fixture).await?;
    assert_eq!(notices(&fixture).await?.len(), 1);
    // Register A's actual read prefix before the final project reports arrive.
    assert!(
        matches!(feed(&fixture,before+2).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,head,..}) if records.is_empty()&&head.get()==u64::try_from(before+2)?)
    );
    assert_eq!(
        report(&fixture, PROJECT).await?,
        ArchiveReportOutcome::Recorded
    );
    drain(&fixture).await?;
    assert_eq!(head(&fixture).await?, before + 2);
    assert_eq!(notices(&fixture).await?.len(), 1);
    assert_eq!(
        report(&fixture, SECOND).await?,
        ArchiveReportOutcome::Recorded
    );
    drain(&fixture).await?;
    assert_eq!(head(&fixture).await?, before + 3);
    assert_eq!(
        notices(&fixture).await?,
        vec![
            Notice {
                position: 1,
                kind: "project_list_changed".into(),
                project: None,
                epoch: None
            },
            Notice {
                position: 2,
                kind: "project_list_changed".into(),
                project: None,
                epoch: None
            },
        ]
    );
    assert!(
        matches!(feed(&fixture,before+2).await?,BoardReadPage::FleetFeed(FleetFeedRead::Page {records,head,..}) if records.len()==1&&head.get()==u64::try_from(before+3)?)
    );
    assert_eq!(
        report(&fixture, SECOND).await?,
        ArchiveReportOutcome::AlreadyRecorded
    );
    drain(&fixture).await?;
    assert_eq!(head(&fixture).await?, before + 3);
    assert_eq!(notices(&fixture).await?.len(), 2);
    fixture.close().await
}
