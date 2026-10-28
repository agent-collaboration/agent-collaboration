//! The real archive owner follows a completed fresh read, never an in-flight stale one.
use super::*;
use futures_util::FutureExt;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn archive_observer_waits_for_a_fresh_sync_while_admission_is_busy() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    let mut archive_reports = runtime.central.archive_reports.subscribe();
    runtime.fleet.synchronize().await?;
    let (entered, release) = runtime.central.hold_next_fleet_read().await;
    let fleet = runtime.fleet.clone();
    let syncing = tokio::spawn(async move { fleet.synchronize().await });
    timeout(BOUND, entered).await??;

    // The first reply has already captured Open. Only a later read can see Archiving.
    let mut records = runtime.fleet.records().await?;
    let mut board = None;
    for record in &mut records {
        if let Record::Board(value) = record {
            assert_eq!(value.state, BoardState::Open);
            value.state = BoardState::Archiving;
            board = Some(value.board_id.clone());
        }
    }
    let board = board.ok_or("fixture board is absent")?;
    runtime
        .central
        .freeze_fleet_snapshot(FleetRecordSnapshot {
            through: FleetPosition::try_from(10)?,
            records,
        })
        .await?;

    // These are the existing archive owner's two stages: fresh Fleet read, then native fact.
    let mut observer_sync = Box::pin(runtime.fleet.synchronize());
    assert!(
        observer_sync.as_mut().now_or_never().is_none(),
        "archive observer completed synchronization while the stale reply was held"
    );
    assert!(archive_reports.borrow().is_empty());
    release.send(()).map_err(|_| "held Fleet read ended")?;
    timeout(BOUND, observer_sync).await??;
    super::super::super::archive_observation::observe_acknowledged_project(
        &runtime.projects,
        &Arc::downgrade(&runtime.admission),
        &Arc::downgrade(&runtime.fleet),
        &runtime.project,
    )
    .await?;
    timeout(BOUND, syncing).await???;
    let store = runtime
        .projects
        .registry()?
        .existing_project(&runtime.project)
        .await?
        .ok_or("fixture native project is absent")?;
    let mut read = store.snapshot().await?;
    let observed = read.records(RecordKind::Fact).await?;
    read.finish().await?;
    let archive_facts = observed
        .into_iter()
        .filter_map(|record| match record {
            Record::Fact(fact)
                if matches!(&fact.kind,
                    FactKind::Lifecycle(LifecycleFact::ArchiveSeen { board: actual }) if actual == &board) => Some(fact),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        archive_facts.len(),
        1,
        "the archive observer must record its native fact after the fresh synchronization"
    );
    let fact_position = archive_facts
        .first()
        .and_then(|fact| fact.envelope.position.project_cursor())
        .ok_or("archive fact cursor is absent")?;
    let report = {
        let reports = timeout(
            BOUND,
            archive_reports.wait_for(|reports| {
                reports
                    .iter()
                    .any(|report| report.board == board && report.project == runtime.project)
            }),
        )
        .await??;
        reports
            .iter()
            .find(|report| report.board == board && report.project == runtime.project)
            .cloned()
            .ok_or("archive report is absent")?
    };
    let (_, acknowledged) = runtime
        .projects
        .acknowledged_project_copy(&runtime.project)
        .await?;
    assert!(acknowledged.is_some_and(|position| position >= report.position));
    let reported_through = collaboration_runtime::project_replication_cursor(
        &store,
        report.position,
        fact_position.epoch,
    )
    .await?;
    assert_eq!(reported_through.epoch, fact_position.epoch);
    assert!(
        reported_through.position >= fact_position.position,
        "the reported copy position must cover the native archive fact"
    );
    drop(store);
    runtime.close().await
}
