#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject wrong behavior; Result propagates real fixture and storage failures."
)]
//! The admission contract uses real SQLite transactions and owning readbacks.
use automation_storage::{AutomationStore, CentralFiringRequest, CentralLocalWork};
use collaboration_protocol::{AutomationContent, BatchId, FiringEntry, FiringOutcome};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn schedule_firing() -> TestResult<(FiringEntry, AutomationContent)> {
    let firing: FiringEntry = serde_json::from_value(serde_json::json!({
        "firingId": uuid::Uuid::now_v7().to_string(), "automationId": uuid::Uuid::now_v7().to_string(),
        "dueAt": 300000, "revision": uuid::Uuid::now_v7().to_string(),
        "content": {"contentId":uuid::Uuid::now_v7().to_string(),"revisionId":uuid::Uuid::now_v7().to_string(),"digest":"a".repeat(64)},
        "work":{"kind":"schedule","definition":{
            "instructionId":uuid::Uuid::now_v7().to_string(),"timing":{"kind":"interval","seconds":600},
            "enabled":true,"destination":{"kind":"freshEachRun","endpoint":{
                "serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"
            },"cwd":"/work/receiver"},"executionTimeoutSeconds":120,"model":"claude-haiku-5-5","effort":"low"
        }}
    }))?;
    let content = AutomationContent {
        reference: firing.content.clone(),
        text: "Frozen first revision".to_owned().try_into()?,
    };
    Ok((firing, content))
}

#[tokio::test]
async fn duplicate_firing_reopens_as_one_preparing_run_with_frozen_inputs() -> TestResult {
    // Arrange: this machine has never held the source schedule or instruction.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (firing, content) = schedule_firing()?;
    let batch_id = BatchId::generate();
    let request = CentralFiringRequest {
        batch_id: batch_id.clone(),
        firing: firing.clone(),
        content,
        now_ms: 400_000,
    };
    // Act: admit, close the owner, reopen, and re-deliver the exact firing.
    let first = store.admit_central_firing(&request).await?;
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    let replay = store.admit_central_firing(&request).await?;
    assert_eq!(
        serde_json::to_value(&first)?,
        serde_json::to_value(&replay)?
    );
    let CentralLocalWork::Schedule { run_id } = first.local_work else {
        return Err("schedule did not yield a run".into());
    };
    let run = store.read_run::<collaboration_protocol::SessionRef, collaboration_protocol::EndpointRef, (), ()>(&run_id).await?;
    assert_eq!(run.phase, agent_automation::RunPhase::Preparing);
    let inputs = run.inputs.ok_or("captured inputs absent")?;
    assert_eq!(inputs.instruction_text.as_str(), "Frozen first revision");
    assert_eq!(inputs.instruction_revision_id, firing.content.revision_id);
    assert_eq!(inputs.schedule_change_id, firing.revision);
    assert!(matches!(replay.outcome, FiringOutcome::Received));
    store.close().await?;
    // Assert: persistence contains one execution, one receipt, one report.
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    for table in ["workflow_runs", "firing_receipts", "central_report_outbox"] {
        let count: i64 = match table {
            "workflow_runs" => {
                sqlx::query_scalar("SELECT count(*) FROM workflow_runs")
                    .fetch_one(&mut connection)
                    .await?
            }
            "firing_receipts" => {
                sqlx::query_scalar("SELECT count(*) FROM firing_receipts")
                    .fetch_one(&mut connection)
                    .await?
            }
            _ => {
                sqlx::query_scalar("SELECT count(*) FROM central_report_outbox")
                    .fetch_one(&mut connection)
                    .await?
            }
        };
        assert_eq!(count, 1, "duplicate effect in {table}");
    }
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn wrong_content_reference_rolls_back_without_receipt_or_run() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (firing, mut content) = schedule_firing()?;
    content.reference.revision_id = agent_automation::RevisionId::generate();
    let request = CentralFiringRequest {
        batch_id: BatchId::generate(),
        firing,
        content,
        now_ms: 400_000,
    };
    assert!(store.admit_central_firing(&request).await.is_err());
    store.close().await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let count: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM firing_receipts)+(SELECT count(*) FROM workflow_runs)+(SELECT count(*) FROM central_report_outbox)").fetch_one(&mut connection).await?;
    assert_eq!(count, 0);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn stored_report_with_a_zero_sequence_is_refused_after_reopen() -> TestResult {
    use collaboration_protocol::{FiringReport, WorkItemReport};
    // Arrange: real admission queued report one, then storage was corrupted coherently.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (firing, content) = schedule_firing()?;
    let mut zero_report = serde_json::to_value(WorkItemReport::Firing(FiringReport {
        firing_id: firing.firing_id.clone(),
        seq: 1_u64.try_into()?,
        outcome: FiringOutcome::Received,
    }))?;
    *zero_report
        .pointer_mut("/value/seq")
        .ok_or("published report sequence missing")? = serde_json::json!(0);
    store
        .admit_central_firing(&CentralFiringRequest {
            batch_id: BatchId::generate(),
            firing,
            content,
            now_ms: 400_000,
        })
        .await?;
    store.close().await?;
    let mut corrupt =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE central_report_outbox SET report_seq=0, report_json=?")
        .bind(serde_json::to_string(&zero_report)?)
        .execute(&mut corrupt)
        .await?;
    corrupt.close().await?;

    // Act / Assert: matching JSON and column values must not make zero a valid report.
    let mut store = AutomationStore::open(&path).await?;
    assert!(matches!(
        store.central_report_batch(400_500).await,
        Err(automation_storage::StorageError::InvalidRecord)
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn corrupt_prior_report_sequence_refuses_transition_without_normalizing_storage() -> TestResult
{
    use agent_automation::{NativeEffectEvidence, RunPhase};
    use collaboration_protocol::{
        AcceptedWorkReport, CodexGeneration, EndpointRef, ExecutionDestination, FiringWork,
        SessionRef, WorkReportSubject,
    };
    // Arrange: acknowledge report one before corrupting its receipt counter.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (mut firing, content) = schedule_firing()?;
    let FiringWork::Schedule { definition } = &mut firing.work else {
        return Err("schedule fixture absent".into());
    };
    let ExecutionDestination::FreshEachRun { endpoint, .. } = &mut definition.destination else {
        return Err("fresh execution fixture absent".into());
    };
    endpoint.endpoint_id = "codex-local".to_owned().try_into()?;
    definition.model = Some("gpt-5.6-luna".to_owned());
    let receipt = store
        .admit_central_firing(&CentralFiringRequest {
            batch_id: BatchId::generate(),
            firing: firing.clone(),
            content,
            now_ms: 400_000,
        })
        .await?;
    let CentralLocalWork::Schedule { run_id } = receipt.local_work else {
        return Err("schedule run absent".into());
    };
    store
        .acknowledge_central_reports(&[AcceptedWorkReport {
            subject: WorkReportSubject::Firing {
                firing_id: firing.firing_id.clone(),
            },
            seq: 1_u64.try_into()?,
        }])
        .await?;
    store.close().await?;
    let mut corrupt =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE firing_receipts SET report_seq=0 WHERE firing_id=?")
        .bind(firing.firing_id.as_str())
        .execute(&mut corrupt)
        .await?;
    corrupt.close().await?;

    // Act: a genuine known-no-effect rejection takes the normal preparation-failure path.
    let mut store = AutomationStore::open(&path).await?;
    let effects: NativeEffectEvidence<SessionRef, CodexGeneration> = serde_json::from_value(
        serde_json::json!({
            "target":null,"generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1},
            "clientUserMessageId":null,"nativeTurnId":null,"nativeSubmissionId":null,
            "allocation":"rejected","resume":"notRequested","submission":"notDispatched","cessation":"notApplicable"
        }),
    )?;
    let result = Box::pin(
        store.fail_run_preparation::<SessionRef, EndpointRef, CodexGeneration, ()>(
            automation_storage::RunPreparationFailure {
                run_id: run_id.clone(),
                effects: effects.into(),
                explanation: "Native allocation rejected".to_owned(),
                now_ms: 400_001,
            },
        ),
    )
    .await;
    if !matches!(result, Err(automation_storage::StorageError::InvalidRecord)) {
        return Err(
            format!("corrupt prior report counter admitted a transition: {result:?}").into(),
        );
    }

    // Assert: the whole owner transaction rolls back; corruption remains explicitly invalid.
    let run = store
        .read_run::<SessionRef, EndpointRef, CodexGeneration, ()>(&run_id)
        .await?;
    if run.phase != RunPhase::Preparing || run.worker_outcome.is_some() {
        return Err("invalid receipt counter partially committed its run transition".into());
    }
    if !matches!(
        store.central_report_batch(400_501).await?,
        automation_storage::CentralReportBatch::Empty
    ) {
        return Err("invalid receipt counter queued a normalized report".into());
    }
    store.close().await?;
    let mut connection =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let sequence: i64 =
        sqlx::query_scalar("SELECT report_seq FROM firing_receipts WHERE firing_id=?")
            .bind(firing.firing_id.as_str())
            .fetch_one(&mut connection)
            .await?;
    if sequence != 0 {
        return Err("stored invalid counter was silently normalized".into());
    }
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn report_deadline_and_exact_acknowledgement_survive_restart() -> TestResult {
    use automation_storage::CentralReportBatch;
    use collaboration_protocol::{AcceptedWorkReport, WorkItemReport, WorkReportSubject};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let mut subjects = Vec::new();
    // 99 real receipt transactions remain queued until the oldest row's deadline.
    for _ in 0..99 {
        let (firing, content) = schedule_firing()?;
        subjects.push(firing.firing_id.clone());
        store
            .admit_central_firing(&CentralFiringRequest {
                batch_id: BatchId::generate(),
                firing,
                content,
                now_ms: 400_000,
            })
            .await?;
    }
    assert!(matches!(
        store.central_report_batch(400_499).await?,
        CentralReportBatch::Waiting { due_at_ms: 400_500 }
    ));
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    let CentralReportBatch::Ready(before) = store.central_report_batch(400_500).await? else {
        return Err("oldest report deadline was lost on restart".into());
    };
    assert_eq!(before.len(), 99);
    // An acknowledgement for another sequence never deletes the current report.
    store
        .acknowledge_central_reports(&[AcceptedWorkReport {
            subject: WorkReportSubject::Firing {
                firing_id: subjects.first().ok_or("subject absent")?.clone(),
            },
            seq: 2_u64.try_into()?,
        }])
        .await?;
    let CentralReportBatch::Ready(after) = store.central_report_batch(400_501).await? else {
        return Err("unacknowledged reports disappeared".into());
    };
    assert_eq!(after.len(), 99);
    for (left, right) in before.iter().zip(&after) {
        assert_eq!(left.report_id, right.report_id);
        assert_eq!(
            serde_json::to_value(&left.report)?,
            serde_json::to_value(&right.report)?
        );
    }
    let first = &before.first().ok_or("report absent")?.report;
    let WorkItemReport::Firing(first) = first else {
        return Err("wrong report kind".into());
    };
    store
        .acknowledge_central_reports(&[AcceptedWorkReport {
            subject: WorkReportSubject::Firing {
                firing_id: first.firing_id.clone(),
            },
            seq: first.seq,
        }])
        .await?;
    let CentralReportBatch::Ready(remaining) = store.central_report_batch(400_502).await? else {
        return Err("partial ack lost reports".into());
    };
    assert_eq!(remaining.len(), 98);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn hundredth_report_is_ready_before_the_sparse_deadline() -> TestResult {
    use automation_storage::CentralReportBatch;
    let directory = tempfile::tempdir()?;
    let mut store = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    for _ in 0..100 {
        let (firing, content) = schedule_firing()?;
        store
            .admit_central_firing(&CentralFiringRequest {
                batch_id: BatchId::generate(),
                firing,
                content,
                now_ms: 400_000,
            })
            .await?;
    }
    let CentralReportBatch::Ready(reports) = store.central_report_batch(400_000).await? else {
        return Err("100 reports waited for the timer".into());
    };
    assert_eq!(reports.len(), 100);
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn two_connections_share_one_firing_receipt_and_work_row() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut first = AutomationStore::open(&path).await?;
    let mut second = AutomationStore::open(&path).await?;
    let (firing, content) = schedule_firing()?;
    let request = CentralFiringRequest {
        batch_id: BatchId::generate(),
        firing,
        content,
        now_ms: 400_000,
    };
    let (left, right) = tokio::join!(
        first.admit_central_firing(&request),
        second.admit_central_firing(&request)
    );
    assert_eq!(serde_json::to_value(left?)?, serde_json::to_value(right?)?);
    first.close().await?;
    second.close().await?;
    Ok(())
}

#[tokio::test]
async fn expired_wake_does_not_block_a_schedule_in_the_same_batch() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    let (mut wake, content) = schedule_firing()?;
    wake.work = serde_json::from_value(
        serde_json::json!({"kind":"wake","createdBy":{"kind":"owner"},"timing":{"kind":"at","at":"1970-01-01T00:05:00Z"},"anchorAt":"1970-01-01T00:00:00Z","createdAt":"1970-01-01T00:00:00Z","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"},"sessionId":"expired-target"},"origin":{"kind":"router"},"delivery":"auto","generationGuard":null,"expiresAt":"1970-01-01T00:06:00Z"}),
    )?;
    let batch_id = BatchId::generate();
    let expired = store
        .admit_central_firing(&CentralFiringRequest {
            batch_id: batch_id.clone(),
            firing: wake,
            content,
            now_ms: 400_000,
        })
        .await?;
    assert!(matches!(expired.local_work, CentralLocalWork::ExpiredWake));
    assert!(matches!(expired.outcome, FiringOutcome::Expired));
    let (firing, content) = schedule_firing()?;
    let schedule = store
        .admit_central_firing(&CentralFiringRequest {
            batch_id,
            firing,
            content,
            now_ms: 400_000,
        })
        .await?;
    assert!(matches!(
        schedule.local_work,
        CentralLocalWork::Schedule { .. }
    ));
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn corrupt_receipt_and_authority_are_refused_on_read() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (firing, content) = schedule_firing()?;
    let schedule_id: agent_automation::ScheduleId =
        firing.automation_id.as_str().to_owned().try_into()?;
    store
        .admit_central_firing(&CentralFiringRequest {
            batch_id: BatchId::generate(),
            firing: firing.clone(),
            content,
            now_ms: 400_000,
        })
        .await?;
    let mut corrupt =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE firing_receipts SET state='{\"state\":\"delivered\"}' WHERE firing_id=?")
        .bind(firing.firing_id.as_str())
        .execute(&mut corrupt)
        .await?;
    assert!(store.read_central_firing(&firing.firing_id).await.is_err());
    sqlx::query("UPDATE schedule_definitions SET authority='unexpected' WHERE schedule_id=?")
        .bind(schedule_id.as_str())
        .execute(&mut corrupt)
        .await?;
    assert!(store.inspect_schedule::<collaboration_protocol::SessionRef,collaboration_protocol::EndpointRef>(&schedule_id).await.is_err());
    corrupt.close().await?;
    store.close().await?;
    assert!(AutomationStore::open(&path).await.is_err());
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn cancel_at_before_commit_rolls_back_projection_work_receipt_and_report() -> TestResult {
    use automation_storage::{
        AutomationFaultEvidence, AutomationFaultObserver, AutomationFaultPoint,
    };
    use std::{future::Future, pin::Pin, sync::Arc};
    use tokio::sync::{Mutex, mpsc};
    struct HoldBeforeCommit(mpsc::UnboundedSender<()>);
    impl AutomationFaultObserver for HoldBeforeCommit {
        fn hold(
            &self,
            point: AutomationFaultPoint,
            _: AutomationFaultEvidence,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async move {
                assert_eq!(point, AutomationFaultPoint::EntryAdmissionBeforeCommit);
                let _ = self.0.send(());
                std::future::pending().await
            })
        }
    }
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (held, mut ready) = mpsc::unbounded_channel();
    store.set_automation_faults(Arc::new(HoldBeforeCommit(held)));
    let store = Arc::new(Mutex::new(store));
    let (firing, content) = schedule_firing()?;
    let firing_id = firing.firing_id.clone();
    let running = Arc::clone(&store);
    let task = tokio::spawn(async move {
        running
            .lock()
            .await
            .admit_central_firing(&CentralFiringRequest {
                batch_id: BatchId::generate(),
                firing,
                content,
                now_ms: 400_000,
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), ready.recv())
        .await?
        .ok_or("admission checkpoint absent")?;
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    Arc::try_unwrap(store)
        .map_err(|_| "writer still owned")?
        .into_inner()
        .close()
        .await?;
    let mut reopened = AutomationStore::open(&path).await?;
    assert!(reopened.read_central_firing(&firing_id).await?.is_none());
    reopened.close().await?;
    let mut check =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM schedule_definitions)+(SELECT count(*) FROM workflow_runs)+(SELECT count(*) FROM firing_receipts)+(SELECT count(*) FROM central_report_outbox)").fetch_one(&mut check).await?;
    assert_eq!(count, 0);
    check.close().await?;
    Ok(())
}

#[tokio::test]
async fn central_projection_rejects_corrupt_off_grid_timing_on_decode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let (firing, content) = schedule_firing()?;
    let schedule_id: agent_automation::ScheduleId =
        firing.automation_id.as_str().to_owned().try_into()?;
    store
        .admit_central_firing(&CentralFiringRequest {
            batch_id: BatchId::generate(),
            firing,
            content,
            now_ms: 400_000,
        })
        .await?;
    let mut corrupt =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE schedule_definitions SET definition_json=json_set(definition_json,'$.timing.seconds',420) WHERE schedule_id=?").bind(schedule_id.as_str()).execute(&mut corrupt).await?;
    let read=store.inspect_schedule::<collaboration_protocol::SessionRef,collaboration_protocol::EndpointRef>(&schedule_id).await;
    assert!(
        read.is_err(),
        "central projection accepted a corrupted seven-minute period"
    );
    corrupt.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn replacement_wake_does_not_block_the_already_sent_wakes_delivery() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    // W1's firing crossed central's cutoff, then W2 replaced its definition before delivery.
    // Both owed firings must be admitted; definition-slot ownership remains in central.
    for _ in 0..2 {
        let (mut firing, content) = schedule_firing()?;
        firing.work = serde_json::from_value(
            serde_json::json!({"kind":"wake","createdBy":{"kind":"owner"},"timing":{"kind":"at","at":"1970-01-01T00:05:00Z"},"anchorAt":"1970-01-01T00:00:00Z","createdAt":"1970-01-01T00:00:00Z","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"},"sessionId":"same-recipient"},"origin":{"kind":"router"},"delivery":"auto","generationGuard":null,"expiresAt":"1970-01-01T00:20:00Z"}),
        )?;
        let receipt = store
            .admit_central_firing(&CentralFiringRequest {
                batch_id: BatchId::generate(),
                firing,
                content,
                now_ms: 400_000,
            })
            .await?;
        assert!(matches!(receipt.local_work, CentralLocalWork::Wake { .. }));
    }
    store.close().await?;
    Ok(())
}
