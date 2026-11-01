//! Drops real central replies after completion while retaining the same machine session.
use super::*;
use collaboration_runtime::{
    CentralAutomationError, CentralAutomationReceiver, CentralReportWorker,
};
use std::sync::atomic::AtomicUsize;
use tokio::task::JoinHandle;

struct LostCustodyReplies {
    transport: Arc<SessionAutomationTransport>,
    fetches: AtomicUsize,
    acknowledgements: AtomicUsize,
}
impl CentralAutomationTransport for LostCustodyReplies {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let answer = self.transport.call(request.clone()).await?;
            let discard = match request {
                AutomationCall::FetchContents(_) => {
                    self.fetches.fetch_add(1, Ordering::SeqCst) == 0
                }
                AutomationCall::AckAutomationBatches(_) => {
                    self.acknowledgements.fetch_add(1, Ordering::SeqCst) == 0
                }
                _ => false,
            };
            if discard {
                Err(CentralAutomationError::OutcomeUnknown)
            } else {
                Ok(answer)
            }
        })
    }
}

// The session is real, but automatic notice admission is intentionally unattached:
// this case owns the sole receiver and injects loss only at its reply carrier.
struct CustodySession {
    transport: Arc<SessionAutomationTransport>,
    admission: Arc<ProjectAdmission>,
    projects: Arc<super::super::super::project_writers::ProjectWriters>,
    running: super::super::super::machine_session::RunningMachineSession,
}
impl CustodySession {
    async fn open(root: &Path, central: &TestCentral) -> ProofResult<Self> {
        let board =
            open_board_admission(root, identity()?, Some(central.session_inputs()?)).await?;
        let session = board.session.as_ref().ok_or("machine session missing")?;
        let ready = session.welcomed.subscribe();
        let transport = Arc::new(SessionAutomationTransport::new(session.requests.clone()));
        let projects = session.projects.clone();
        let admission = board.admission.clone();
        let running = board.start().ok_or("machine session not started")?;
        let host = Self {
            transport,
            admission,
            projects,
            running,
        };
        if let Err(error) = wait_welcome(ready).await {
            host.close().await?;
            return Err(error);
        }
        Ok(host)
    }
    async fn close(self) -> ProofResult {
        drop(self.transport);
        let stopped = self.running.shutdown().await;
        drop(self.projects);
        let closed = Arc::try_unwrap(self.admission)
            .map_err(|_| "project admission still owned")?
            .close()
            .await;
        stopped?;
        closed?;
        Ok(())
    }
}

type ReceivingTask = JoinHandle<Result<(), CentralAutomationError>>;
async fn require_retry(
    task: &mut Option<ReceivingTask>,
    clock: &ControlledClock,
    deadline: i64,
) -> ProofResult {
    let receiving = task.as_mut().ok_or("receiver already completed")?;
    let completed = tokio::select! {
        result = receiving => Some(result),
        registered = clock.wait_for_deadline(deadline) => {
            registered?;
            None
        }
    };
    if let Some(result) = completed {
        task.take();
        return Err(format!("receiver abandoned recoverable custody: {result:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn central_automation_retries_lost_content_and_ack_replies_on_the_live_session() -> TestResult
{
    // Arrange: actual central custody and local SQLite, with reply loss outside both.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let now = chrono::DateTime::parse_from_rfc3339(DUE)?.timestamp_millis();
    let central = start_central(&directory.path().join("central")).await?;
    let host = CustodySession::open(&root, &central).await?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&root.join("automation.sqlite")).await?,
    ));
    let clock = Arc::new(ControlledClock::new(now));
    let losses = Arc::new(LostCustodyReplies {
        transport: host.transport.clone(),
        fetches: AtomicUsize::new(0),
        acknowledgements: AtomicUsize::new(0),
    });
    let receiver = Arc::new(CentralAutomationReceiver::new(
        store.clone(),
        losses.clone(),
        clock.clone(),
        identity()?.service_id().clone(),
    ));
    let mut receiving = None;
    let proof: ProofResult =
        async {
            require(
                pending(&host.transport).await?.is_empty(),
                "fresh custody exists",
            )?;
            create_schedule(
                &host.transport,
                collaboration_protocol::TimingRequest::At {
                    at: DUE.to_owned().try_into()?,
                },
                "Lost reply must retain the frozen work",
            )
            .await?;
            advance(&central, DUE).await?;
            let retained = pending(&host.transport).await?;
            require(retained.len() == 1, "central did not freeze one Batch")?;
            let batch: AutomationBatch = serde_json::from_slice(&retained[0].frozen_bytes)?;
            let Some(WorkEntry::Firing(firing)) = batch.entries.entries().first() else {
                return Err("frozen Firing missing".into());
            };

            // Act: lose FetchContents, then the ACK answer after central commits it.
            let receiving_owner = receiver.clone();
            receiving = Some(tokio::spawn(async move {
                receiving_owner.receive_pending().await
            }));
            timeout(BOUND, require_retry(&mut receiving, &clock, now + 1000)).await??;

            // Assert: the first loss leaves central custody intact and no local receipt.
            let still_owed = pending(&host.transport).await?;
            require(
                still_owed.len() == 1
                    && still_owed[0].batch_id == retained[0].batch_id
                    && still_owed[0].frozen_bytes == retained[0].frozen_bytes,
                "lost content reply changed central custody",
            )?;
            require(
                store
                    .lock()
                    .await
                    .read_central_firing(&firing.firing_id)
                    .await?
                    .is_none(),
                "lost content reply admitted unavailable bytes",
            )?;
            clock.advance(now + 1000);
            timeout(BOUND, require_retry(&mut receiving, &clock, now + 2000)).await??;
            let receipt = store
                .lock()
                .await
                .read_central_firing(&firing.firing_id)
                .await?
                .ok_or("ACK was sent before the durable receipt")?;
            require(
                receipt.batch_id == retained[0].batch_id,
                "receipt Batch identity changed",
            )?;
            require(
                pending(&host.transport).await?.is_empty(),
                "real ACK was not committed",
            )?;
            let acknowledged = show(&host.transport, &firing.firing_id).await?;
            require(
                matches!(acknowledged.outcome, FiringOutcome::Received)
                    && acknowledged.report_seq.is_none(),
                "ACK reply loss fabricated a report",
            )?;
            clock.advance(now + 2000);
            let task = receiving.as_mut().ok_or("receiver missing")?;
            let completed = timeout(BOUND, task).await?;
            receiving.take();
            completed??;
            require(
                losses.fetches.load(Ordering::SeqCst) == 2
                    && losses.acknowledgements.load(Ordering::SeqCst) == 1,
                "custody did not reconcile the two lost replies",
            )?;
            use sqlx::Connection;
            let mut sql = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
            )
            .await?;
            let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM workflow_runs),(SELECT count(*) FROM firing_receipts)",
        ).fetch_one(&mut sql).await?;
            sql.close().await?;
            require(
                counts == (1, 1),
                "reply recovery duplicated or dropped local work",
            )?;
            let reports =
                CentralReportWorker::new(store.clone(), host.transport.clone(), clock.clone());
            require(
                reports.flush_due().await?.is_none(),
                "admission report remains owed",
            )?;
            let reported = show(&host.transport, &firing.firing_id).await?;
            require(
                reported.report_seq.map(u64::from) == Some(1),
                "report sequence changed",
            )?;
            require(
                serde_json::to_value(reported.outcome)? == serde_json::to_value(receipt.outcome)?,
                "recovered receipt outcome changed",
            )?;
            Ok(())
        }
        .await;
    if let Some(task) = receiving {
        task.abort();
        let _cancelled = task.await;
    }
    drop(receiver);
    drop(losses);
    let closed = Arc::try_unwrap(store)
        .map_err(|_| "automation store still owned")?
        .into_inner()
        .close()
        .await;
    let stopped = host.close().await;
    let central_stopped = central.shutdown().await;
    proof?;
    closed?;
    stopped?;
    central_stopped?;
    Ok(())
}
