//! Deterministic adapter proof: real SQLite/operations and the existing clock, supplied S2 answers.
//! Actual central admission/report persistence remains a separate integration gate.
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, CentralWakeSnapshot, OperationId, WakeWaitRequest, WakeupId,
};
use collaboration_runtime::{CentralAutomationError, CentralAutomationTransport};
use collaboration_service::{
    SubscriptionClock, WakeOperationFailure, WakeWaitResponse,
    collaboration_application::WakeOperations,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Semaphore, mpsc, oneshot, watch},
    time::Instant,
};

type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Clone, Copy)]
struct ClockPosition {
    wall: chrono::DateTime<chrono::Utc>,
    monotonic: Instant,
}
struct ObservedClock {
    position: watch::Sender<ClockPosition>,
    sleeps: mpsc::UnboundedSender<Instant>,
}
impl ObservedClock {
    fn advance(&self, elapsed: Duration) -> ProofResult {
        let wall_elapsed = chrono::Duration::from_std(elapsed)?;
        self.position.send_modify(|position| {
            position.wall += wall_elapsed;
            position.monotonic += elapsed;
        });
        Ok(())
    }
}
impl SubscriptionClock for ObservedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.position.borrow().wall
    }
    fn monotonic_now(&self) -> Instant {
        self.position.borrow().monotonic
    }
    fn sleep_until(&self, deadline: Instant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let _ = self.sleeps.send(deadline);
        let mut changes = self.position.subscribe();
        Box::pin(async move {
            loop {
                let now = changes.borrow().monotonic;
                if now >= deadline {
                    return;
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}
struct ReadRequest {
    wakeup_id: WakeupId,
    answer: oneshot::Sender<Result<AutomationAnswer, CentralAutomationError>>,
}
struct SuppliedCentral {
    requests: mpsc::UnboundedSender<ReadRequest>,
    count: Arc<AtomicUsize>,
    unexpected: Arc<AtomicUsize>,
}
impl CentralAutomationTransport for SuppliedCentral {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>
    {
        Box::pin(async move {
            let AutomationCall::WakeShow(request) = request else {
                self.unexpected.fetch_add(1, Ordering::SeqCst);
                return Err(CentralAutomationError::InvalidWork);
            };
            self.count.fetch_add(1, Ordering::SeqCst);
            let (answer, received) = oneshot::channel();
            self.requests
                .send(ReadRequest {
                    wakeup_id: request.wakeup_id,
                    answer,
                })
                .map_err(|_| CentralAutomationError::Unavailable)?;
            received
                .await
                .map_err(|_| CentralAutomationError::Unavailable)?
        })
    }
}
struct WaitFixture {
    tasks: tokio::task::JoinSet<Result<WakeWaitResponse, WakeOperationFailure>>,
    operations: Arc<WakeOperations>,
    store: Arc<Mutex<automation_storage::AutomationStore>>,
    directory: tempfile::TempDir,
    clock: Arc<ObservedClock>,
    sleeps: mpsc::UnboundedReceiver<Instant>,
    reads: mpsc::UnboundedReceiver<ReadRequest>,
    count: Arc<AtomicUsize>,
    unexpected: Arc<AtomicUsize>,
    id: WakeupId,
    origin: Instant,
    deadline: Option<Instant>,
}
impl WaitFixture {
    async fn open() -> ProofResult<Self> {
        let directory = tempfile::Builder::new()
            .prefix("s6-wait-")
            .tempdir_in("/tmp")?;
        let store = Arc::new(Mutex::new(
            automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
                .await?,
        ));
        let origin = Instant::now();
        let (position, _) = watch::channel(ClockPosition {
            wall: "2026-11-01T12:00:00Z".parse()?,
            monotonic: origin,
        });
        let (sleep_sender, sleeps) = mpsc::unbounded_channel();
        let clock = Arc::new(ObservedClock {
            position,
            sleeps: sleep_sender,
        });
        let (requests, reads) = mpsc::unbounded_channel();
        let count = Arc::new(AtomicUsize::new(0));
        let unexpected = Arc::new(AtomicUsize::new(0));
        let transport = Arc::new(SuppliedCentral {
            requests,
            count: count.clone(),
            unexpected: unexpected.clone(),
        });
        let service = SERVICE.to_owned().try_into()?;
        let operations = Arc::new(
            WakeOperations::new(&service, Some(&store))
                .with_catalog(Some(transport))
                .with_clock(clock.clone())
                .with_wait_capacity(&Arc::new(Semaphore::new(1))),
        );
        Ok(Self {
            tasks: tokio::task::JoinSet::new(),
            operations,
            store,
            directory,
            clock,
            sleeps,
            reads,
            count,
            unexpected,
            id: WakeupId::generate(),
            origin,
            deadline: None,
        })
    }
    fn start(&mut self, timeout_seconds: u32) {
        self.deadline = Some(self.origin + Duration::from_secs(u64::from(timeout_seconds)));
        let operations = self.operations.clone();
        let request = WakeWaitRequest {
            wakeup_id: self.id.clone(),
            after: None,
            timeout_seconds: Some(timeout_seconds),
        };
        self.tasks
            .spawn(async move { operations.wake_wait_until_first_fire(request).await });
    }
    async fn read(&mut self) -> ProofResult<ReadRequest> {
        let request = self
            .reads
            .recv()
            .await
            .ok_or("wait did not retain its read producer")?;
        if request.wakeup_id != self.id {
            return Err("wait substituted another Wake ID".into());
        }
        Ok(request)
    }
    async fn sleeping_until(&mut self, elapsed: Duration) -> ProofResult {
        let expected = self.origin + elapsed;
        while let Some(deadline) = self.sleeps.recv().await {
            if deadline == expected {
                return Ok(());
            }
            if Some(deadline) != self.deadline {
                return Err("wait registered a read before its known due/backoff deadline".into());
            }
        }
        Err("wait never registered its due/deadline sleep".into())
    }
    async fn finish(&mut self) -> ProofResult<WakeWaitResponse> {
        let joined = self.tasks.join_next().await.ok_or("wait task missing")?;
        Ok(joined??)
    }
    async fn close(mut self) -> ProofResult {
        self.tasks.shutdown().await;
        drop(self.operations);
        Arc::try_unwrap(self.store)
            .map_err(|_| "wait still owns SQLite")?
            .into_inner()
            .close()
            .await?;
        self.directory.close()?;
        if self.unexpected.load(Ordering::SeqCst) != 0 {
            return Err("a wait made a central call other than WakeShow".into());
        }
        Ok(())
    }
    fn wake_snapshot(&self, state: &str) -> ProofResult<CentralWakeSnapshot> {
        Ok(serde_json::from_value(serde_json::json!({
            "definition":{"wakeupId":self.id,"changeId":OperationId::generate(),"createdBy":{"kind":"router"},
                "message":{"target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"inert-wait-target"},
                    "content":{"kind":"router","text":"No native backend or input"},"delivery":"queue"},
                "timing":{"kind":"at","at":"2026-11-01T12:05:00Z"},"anchorAt":"2026-11-01T11:00:00Z",
                "createdAt":"2026-11-01T11:00:00Z","expiresAt":if state == "expired" { "2026-11-01T12:00:00Z" } else { "2026-11-01T13:00:00Z" }},
            "state":state,"nextDueAt":if state == "active" { Some("2026-11-01T12:05:00Z") } else { None },"firstFiring":null
        }))?)
    }
}

async fn guarded(proof: impl Future<Output = ProofResult>) -> ProofResult {
    // Failure guard only; every assertion is driven by retained read/sleep/clock facts.
    tokio::time::timeout(Duration::from_secs(20), proof)
        .await
        .map_err(|_| "controlled wait facts did not complete within the failure guard")?
}

#[tokio::test]
async fn timeout_before_due_returns_only_the_known_wake_id_without_polling() -> ProofResult {
    guarded(async {
        let mut fixture = WaitFixture::open().await?;
        fixture.start(60);
        let read = fixture.read().await?;
        read.answer.send(Ok(AutomationAnswer::Wake(fixture.wake_snapshot("active")?))).map_err(|_| "read dropped")?;
        fixture.sleeping_until(Duration::from_secs(60)).await?;
        fixture.clock.advance(Duration::from_secs(60))?;
        let result = serde_json::to_value(fixture.finish().await?)?;
        if result != serde_json::json!({"kind":"central","value":{"wakeupId":fixture.id,"outcome":{"kind":"pending"}}})
            || fixture.count.load(Ordering::SeqCst) != 1 {
            return Err("pre-due timeout polled, fabricated evidence, or lost the actual Wake ID".into());
        }
        fixture.close().await
    }).await
}

#[tokio::test]
async fn sleep_to_due_then_held_first_firing_answers_before_any_machine_report() -> ProofResult {
    guarded(async {
        let mut fixture = WaitFixture::open().await?;
        fixture.start(600);
        fixture.read().await?.answer.send(Ok(AutomationAnswer::Wake(fixture.wake_snapshot("active")?))).map_err(|_| "initial read dropped")?;
        fixture.sleeping_until(Duration::from_secs(300)).await?;
        if fixture.count.load(Ordering::SeqCst) != 1 { return Err("central was polled before its first known grid due time".into()); }
        fixture.clock.advance(Duration::from_secs(300))?;
        // The grid deadline arrives before central has admitted its first Firing.
        fixture.read().await?.answer.send(Ok(AutomationAnswer::Wake(fixture.wake_snapshot("active")?))).map_err(|_| "delayed admission read dropped")?;
        fixture.sleeping_until(Duration::from_millis(300_200)).await?;
        if fixture.count.load(Ordering::SeqCst) != 2 { return Err("post-due backoff did not retain the current read position".into()); }
        fixture.clock.advance(Duration::from_millis(200))?;
        let due_ms = chrono::DateTime::parse_from_rfc3339("2026-11-01T12:05:00Z")?.timestamp_millis();
        let firing: collaboration_protocol::FiringSnapshot = serde_json::from_value(serde_json::json!({
            "firingId":"019f0000-0000-7000-8000-000000000001","automationId":fixture.id,
            "dueAt":due_ms,"revision":OperationId::generate(),
            "content":{"contentId":"019f0000-0000-7000-8000-000000000004","revisionId":"019f0000-0000-7000-8000-000000000006","digest":"a".repeat(64)},
            "outcome":{"state":"held"},"reportSeq":null,"batchId":null
        }))?;
        let expected = serde_json::to_value(&firing)?;
        let mut snapshot = fixture.wake_snapshot("finished")?;
        snapshot.first_firing = Some(firing);
        fixture.read().await?.answer.send(Ok(AutomationAnswer::Wake(snapshot))).map_err(|_| "due read dropped")?;
        let result = serde_json::to_value(fixture.finish().await?)?;
        if result.pointer("/value/outcome/kind") != Some(&serde_json::json!("fired")) || result.pointer("/value/outcome/firing") != Some(&expected)
            || fixture.count.load(Ordering::SeqCst) != 3 {
            return Err("wait delayed for a report/delivery or changed the actual first Firing evidence".into());
        }
        fixture.close().await
    }).await
}

#[tokio::test]
async fn lifecycle_without_admission_stays_distinct_from_firing_or_fault() -> ProofResult {
    guarded(async {
        for (state, expected) in [
            ("paused", "paused"),
            ("cancelled", "cancelled"),
            ("expired", "expired"),
            ("finished", "finishedWithoutFiring"),
        ] {
            let mut fixture = WaitFixture::open().await?;
            fixture.start(600);
            fixture
                .read()
                .await?
                .answer
                .send(Ok(AutomationAnswer::Wake(fixture.wake_snapshot(state)?)))
                .map_err(|_| "lifecycle read dropped")?;
            let result = serde_json::to_value(fixture.finish().await?)?;
            if result.pointer("/value/outcome/kind") != Some(&serde_json::json!(expected))
                || fixture.count.load(Ordering::SeqCst) != 1
            {
                return Err(format!(
                    "{state} was not preserved as its actual pre-admission lifecycle outcome"
                )
                .into());
            }
            fixture.close().await?;
        }
        Ok(())
    })
    .await
}

#[tokio::test]
async fn faulted_wake_retains_the_shared_fault_and_never_becomes_finished() -> ProofResult {
    guarded(async {
        let mut fixture = WaitFixture::open().await?;
        fixture.start(600);
        let fault: collaboration_protocol::FaultedAutomationSnapshot = serde_json::from_value(serde_json::json!({
            "automationId":fixture.id,"kind":"wake","revision":OperationId::generate(),
            "fault":{"error":{"kind":"invalidRecord","field":"definition_json"},"faultedAt":"2026-11-01T12:00:00Z"}
        }))?;
        let expected = serde_json::to_value(&fault)?;
        fixture.read().await?.answer.send(Ok(AutomationAnswer::Faulted(fault))).map_err(|_| "fault read dropped")?;
        let result = serde_json::to_value(fixture.finish().await?)?;
        if result["kind"] != "faulted" || result["fault"] != expected || !result["message"].is_string() {
            return Err("faulted wait lost its shared fault/revision or became a lifecycle success".into());
        }
        fixture.close().await
    }).await
}

#[tokio::test]
async fn unanswered_read_deadline_and_lost_read_answer_remain_pending_without_retry() -> ProofResult
{
    guarded(async {
        let mut fixture = WaitFixture::open().await?;
        fixture.start(60);
        let read = fixture.read().await?;
        fixture.clock.advance(Duration::from_secs(60))?;
        let result = serde_json::to_value(fixture.finish().await?)?;
        if result.pointer("/value/outcome/kind") != Some(&serde_json::json!("pending")) || !read.answer.is_closed()
            || fixture.count.load(Ordering::SeqCst) != 1 { return Err("read deadline did not release its task or invented a result".into()); }
        fixture.close().await?;
        let mut fixture = WaitFixture::open().await?;
        fixture.start(60);
        fixture.read().await?.answer.send(Err(CentralAutomationError::OutcomeUnknown)).map_err(|_| "lost read dropped")?;
        let result = serde_json::to_value(fixture.finish().await?)?;
        if result != serde_json::json!({"kind":"central","value":{"wakeupId":fixture.id,"outcome":{"kind":"pending"}}})
            || fixture.count.load(Ordering::SeqCst) != 1 { return Err("lost read was retried or fabricated Firing evidence".into()); }
        fixture.close().await
    }).await
}
