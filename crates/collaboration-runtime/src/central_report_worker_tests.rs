#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject wrong behavior; Result propagates real fixture and storage failures."
)]
//! Controlled time drives the real worker; its reports cross only the authorized S4 stand-in.
use crate::{
    CentralAutomationClock, CentralAutomationError, CentralAutomationTransport, CentralReportWorker,
};
use automation_storage::AutomationStore;
use collaboration_protocol::{
    AcceptedWorkReport, AutomationAnswer, AutomationCall, FiringId, FiringOutcome, FiringReport,
    OperationId, WorkItemReport, WorkReportAcknowledgements, WorkReportSubject,
};
use sqlx::Connection;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct ControlledClock {
    now: watch::Sender<i64>,
    waiting: mpsc::UnboundedSender<i64>,
}
impl CentralAutomationClock for ControlledClock {
    fn now_ms(&self) -> i64 {
        *self.now.borrow()
    }
    fn wait_until(&self, deadline: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut time = self.now.subscribe();
            let _ = self.waiting.send(deadline);
            loop {
                if *time.borrow() >= deadline {
                    return;
                }
                if time.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}
struct ReportStandIn {
    observed: mpsc::UnboundedSender<Vec<WorkItemReport>>,
}
impl CentralAutomationTransport for ReportStandIn {
    fn call(
        &self,
        call: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let AutomationCall::ReportWorkItems(reports) = call else {
                return Err(CentralAutomationError::InvalidReply);
            };
            let mut accepted = Vec::new();
            for report in reports.reports() {
                let WorkItemReport::Firing(report) = report else {
                    return Err(CentralAutomationError::InvalidReply);
                };
                accepted.push(AcceptedWorkReport {
                    subject: WorkReportSubject::Firing {
                        firing_id: report.firing_id.clone(),
                    },
                    seq: report.seq,
                });
            }
            self.observed
                .send(reports.reports().to_vec())
                .map_err(|_| CentralAutomationError::Unavailable)?;
            Ok(AutomationAnswer::Reports(WorkReportAcknowledgements {
                accepted,
            }))
        })
    }
}
#[tokio::test]
async fn silent_sparse_queues_flush_on_the_oldest_deadline_and_hundred_flushes_immediately()
-> TestResult {
    for count in [1, 99, 100] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("automation.sqlite");
        let store = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
        let mut seed = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
        )
        .await?;
        for _ in 0..count {
            let report = WorkItemReport::Firing(FiringReport {
                firing_id: FiringId::generate(),
                seq: 1_u64.try_into()?,
                outcome: FiringOutcome::Received,
            });
            let WorkItemReport::Firing(inner) = &report else {
                return Err("wrong seed kind".into());
            };
            sqlx::query("INSERT INTO central_report_outbox(report_id,subject_kind,subject_id,report_seq,report_json,queued_at_ms) VALUES (?,'firing',?,1,?,400000)")
                .bind(OperationId::generate().as_str()).bind(inner.firing_id.as_str()).bind(serde_json::to_string(&report)?).execute(&mut seed).await?;
        }
        seed.close().await?;
        let (now, _) = watch::channel(400_000);
        let (waiting, mut waits) = mpsc::unbounded_channel();
        let clock = Arc::new(ControlledClock { now, waiting });
        let (observed, mut reports) = mpsc::unbounded_channel();
        let shutdown = CancellationToken::new();
        let worker = CentralReportWorker::new(
            store.clone(),
            Arc::new(ReportStandIn { observed }),
            clock.clone(),
        );
        let task = tokio::spawn(worker.run(shutdown.clone()));
        if count < 100 {
            let deadline = tokio::time::timeout(std::time::Duration::from_secs(3), waits.recv())
                .await?
                .ok_or("clock waiter absent")?;
            assert_eq!(deadline, 400_500);
            assert!(reports.try_recv().is_err());
            clock.now.send_replace(400_499);
            assert!(reports.try_recv().is_err());
            clock.now.send_replace(400_500);
        }
        let batch = tokio::time::timeout(std::time::Duration::from_secs(3), reports.recv())
            .await?
            .ok_or("reports absent")?;
        assert_eq!(batch.len(), count);
        if count == 100 {
            assert_eq!(clock.now_ms(), 400_000);
        }
        shutdown.cancel();
        task.await?;
    }
    Ok(())
}

#[tokio::test]
async fn shutdown_while_a_report_answer_is_unknown_keeps_the_outbox() -> TestResult {
    struct HoldAnswer(mpsc::UnboundedSender<()>);
    impl CentralAutomationTransport for HoldAnswer {
        fn call(
            &self,
            _: AutomationCall,
        ) -> Pin<
            Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>,
        > {
            Box::pin(async move {
                let _ = self.0.send(());
                std::future::pending().await
            })
        }
    }
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let store = Arc::new(Mutex::new(AutomationStore::open(&path).await?));
    let report = WorkItemReport::Firing(FiringReport {
        firing_id: FiringId::generate(),
        seq: 1_u64.try_into()?,
        outcome: FiringOutcome::Received,
    });
    let WorkItemReport::Firing(inner) = &report else {
        return Err("wrong seed".into());
    };
    let mut seed = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await?;
    sqlx::query("INSERT INTO central_report_outbox(report_id,subject_kind,subject_id,report_seq,report_json,queued_at_ms) VALUES (?,'firing',?,1,?,400000)")
        .bind(OperationId::generate().as_str()).bind(inner.firing_id.as_str()).bind(serde_json::to_string(&report)?).execute(&mut seed).await?;
    seed.close().await?;
    let (now, _) = watch::channel(400_500);
    let (waiting, _) = mpsc::unbounded_channel();
    let clock = Arc::new(ControlledClock { now, waiting });
    let (sent, mut observed) = mpsc::unbounded_channel();
    let shutdown = CancellationToken::new();
    let worker = CentralReportWorker::new(store.clone(), Arc::new(HoldAnswer(sent)), clock);
    let mut task = tokio::spawn(worker.run(shutdown.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(3), observed.recv())
        .await?
        .ok_or("report not sent")?;
    shutdown.cancel();
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(3), &mut task).await;
    match stopped {
        Ok(result) => result?,
        Err(_) => {
            task.abort();
            let _ = task.await;
            return Err("report worker ignored shutdown while its answer was unknown".into());
        }
    }
    let pending = store.lock().await.central_report_batch(400_500).await?;
    let automation_storage::CentralReportBatch::Ready(reports) = pending else {
        return Err("unknown report was dropped".into());
    };
    assert_eq!(reports.len(), 1);
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn report_fault_selector_reaches_a_nonfirst_report_before_local_acknowledgement() -> TestResult
{
    use crate::{AutomationFaultPoint, AutomationFaultSelector, DebugAutomationFaultController};

    // Arrange: two real owed rows; select the second in durable send order.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let faults = Arc::new(DebugAutomationFaultController::default());
    store.set_automation_faults(faults.clone());
    let mut seed = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await?;
    for queued_at in [400_000_i64, 400_001] {
        let report = FiringReport {
            firing_id: FiringId::generate(),
            seq: 1_u64.try_into()?,
            outcome: FiringOutcome::Received,
        };
        sqlx::query("INSERT INTO central_report_outbox(report_id,subject_kind,subject_id,report_seq,report_json,queued_at_ms) VALUES (?,'firing',?,1,?,?)")
            .bind(OperationId::generate().as_str()).bind(report.firing_id.as_str())
            .bind(serde_json::to_string(&WorkItemReport::Firing(report.clone()))?)
            .bind(queued_at).execute(&mut seed).await?;
    }
    seed.close().await?;
    let automation_storage::CentralReportBatch::Ready(ordered) =
        store.central_report_batch(400_501).await?
    else {
        return Err("reports not ready".into());
    };
    let Some(WorkItemReport::Firing(selected)) = ordered.last().map(|owed| &owed.report) else {
        return Err("second report absent".into());
    };
    let selected_id = selected.firing_id.clone();
    let arm_id = faults
        .arm(
            AutomationFaultPoint::ReportSentBeforeAcknowledgement,
            AutomationFaultSelector {
                firing_id: Some(selected_id.clone()),
                ..Default::default()
            },
        )
        .await?;
    let store = Arc::new(Mutex::new(store));
    let (now, _) = watch::channel(400_501);
    let (waiting, _) = mpsc::unbounded_channel();
    let (observed, mut sent) = mpsc::unbounded_channel();
    let worker = CentralReportWorker::new(
        store.clone(),
        Arc::new(ReportStandIn { observed }),
        Arc::new(ControlledClock { now, waiting }),
    );

    // Act: the stand-in answers the whole envelope; local deletion must still be held.
    let task = tokio::spawn(async move { worker.flush_due().await });
    let held = tokio::time::timeout(std::time::Duration::from_secs(3), faults.wait_held(&arm_id))
        .await??;
    assert_eq!(held.evidence.firing_id, Some(selected_id));
    assert_eq!(sent.recv().await.ok_or("report envelope absent")?.len(), 2);
    let automation_storage::CentralReportBatch::Ready(owed) =
        store.lock().await.central_report_batch(400_501).await?
    else {
        return Err("local acknowledgement preceded selected hold".into());
    };
    assert_eq!(owed.len(), 2);
    assert!(!task.is_finished());
    faults.release(&arm_id).await?;
    assert!(task.await??.is_none());

    // Assert: the exact accepted rows disappear only after release.
    assert!(matches!(
        store.lock().await.central_report_batch(400_501).await?,
        automation_storage::CentralReportBatch::Empty
    ));
    Ok(())
}
