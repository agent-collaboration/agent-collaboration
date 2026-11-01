//! Retains the real session, central client and storage owners until explicit shutdown.
use super::*;
use std::sync::atomic::AtomicI64;

pub(super) type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
pub(super) const INITIAL: &str = "2026-10-31T10:04:59.999Z";
pub(super) const DUE: &str = "2026-10-31T10:05:00Z";

pub(super) struct ControlledClock {
    now: AtomicI64,
    changed: tokio::sync::watch::Sender<i64>,
    waiting: tokio::sync::watch::Sender<Option<i64>>,
}
impl ControlledClock {
    pub fn new(now: i64) -> Self {
        Self {
            now: AtomicI64::new(now),
            changed: tokio::sync::watch::channel(now).0,
            waiting: tokio::sync::watch::channel(None).0,
        }
    }
    pub fn advance(&self, now: i64) {
        self.now.store(now, Ordering::SeqCst);
        self.changed.send_replace(now);
    }
    pub async fn wait_for_deadline(&self, deadline: i64) -> ProofResult {
        let mut waiting = self.waiting.subscribe();
        while *waiting.borrow_and_update() != Some(deadline) {
            waiting.changed().await?;
        }
        Ok(())
    }
}
impl CentralAutomationClock for ControlledClock {
    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
    fn wait_until(&self, deadline: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut changed = self.changed.subscribe();
            self.waiting.send_replace(Some(deadline));
            while self.now_ms() < deadline {
                if changed.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

pub(super) async fn start_central(root: &Path) -> ProofResult<TestCentral> {
    TestCentral::start_with_peer_and_controls(
        root,
        &origin()?,
        durable_steps::Uuid::now_v7().to_string().parse()?,
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
                now: serde_json::from_value(serde_json::json!(INITIAL))?,
            }),
        }],
    )
    .await
}

pub(super) async fn advance(central: &TestCentral, now: &str) -> ProofResult {
    ConformanceClient::new(central.base_url())?
        .automation_clock(AutomationClockControl::Advance {
            now: serde_json::from_value(serde_json::json!(now))?,
        })
        .await?;
    Ok(())
}

pub(super) struct ReceiverHost {
    pub store: Arc<Mutex<AutomationStore>>,
    pub transport: SessionAutomationTransport,
    pub receiver: Arc<collaboration_runtime::CentralAutomationReceiver>,
    pub reports: collaboration_runtime::CentralReportWorker,
    pub admission: Arc<ProjectAdmission>,
    pub projects: Arc<super::super::super::project_writers::ProjectWriters>,
    pub fleet: Weak<FleetClient>,
    running: super::super::super::machine_session::RunningMachineSession,
}
impl ReceiverHost {
    pub async fn open(
        root: &Path,
        central: &TestCentral,
        clock: Arc<ControlledClock>,
        faults: Option<Arc<DebugAutomationFaultController>>,
    ) -> ProofResult<Self> {
        let mut board =
            open_board_admission(root, identity()?, Some(central.session_inputs()?)).await?;
        let session = board.session.as_ref().ok_or("machine session missing")?;
        let transport = SessionAutomationTransport::new(session.requests.clone());
        let ready = session.welcomed.subscribe();
        let projects = session.projects.clone();
        let fleet = board.fleet.clone();
        let admission = board.admission.clone();
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.join("automation.sqlite")).await?,
        ));
        if let Some(faults) = faults {
            store.lock().await.set_automation_faults(faults);
        }
        let (receiver, reports) = board
            .attach_automations(store.clone(), clock, identity()?.service_id().clone())
            .await
            .ok_or("automation session missing")?;
        let running = board.start().ok_or("machine session not started")?;
        let host = Self {
            store,
            transport,
            receiver,
            reports,
            admission,
            projects,
            fleet,
            running,
        };
        if let Err(error) = wait_welcome(ready).await {
            let closed = host.close().await;
            return Err(match closed {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; cleanup: {cleanup}").into(),
            });
        }
        Ok(host)
    }
    pub async fn close(self) -> ProofResult {
        let Self {
            store,
            transport,
            receiver,
            reports,
            admission,
            projects,
            fleet,
            running,
        } = self;
        drop(transport);
        drop(receiver);
        drop(reports);
        drop(fleet);
        let stopped = running.shutdown().await;
        drop(projects);
        let closed = Arc::try_unwrap(admission)
            .map_err(|_| "project admission still owned")?
            .close()
            .await;
        let local = Arc::try_unwrap(store)
            .map_err(|_| "automation store still owned")?
            .into_inner()
            .close()
            .await;
        stopped?;
        closed?;
        local?;
        Ok(())
    }
}

pub(super) async fn pending(
    transport: &SessionAutomationTransport,
) -> ProofResult<Vec<collaboration_protocol::FrozenAutomationBatch>> {
    let call =
        AutomationCall::PullAutomationBatches(collaboration_protocol::PullAutomationBatches {
            after: None,
            max_bytes: 1_048_576,
            work_protocols: vec![collaboration_protocol::MachineWorkProtocol::MachineWorkV1],
        });
    match transport.call(call).await? {
        AutomationAnswer::Batches(page) => Ok(page.batches),
        other => Err(format!("custody refused: {other:?}").into()),
    }
}
pub(super) async fn create_schedule(
    transport: &SessionAutomationTransport,
    timing: collaboration_protocol::TimingRequest,
    text: &str,
) -> ProofResult<collaboration_protocol::ScheduleSnapshot> {
    use collaboration_protocol::{
        ExecutionDestination, InstructionCreateParams, ScheduleCreateRequest, ScheduleDefinition,
    };
    let instruction = match transport
        .call(AutomationCall::InstructionCreate(InstructionCreateParams {
            operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
            text: text.to_owned().try_into()?,
        }))
        .await?
    {
        AutomationAnswer::Instruction(instruction) => instruction,
        other => return Err(format!("instruction refused: {other:?}").into()),
    };
    let call = AutomationCall::ScheduleCreate(ScheduleCreateRequest {
        operation_id: durable_steps::Uuid::now_v7().to_string().try_into()?,
        definition: ScheduleDefinition {
            instruction_id: instruction.instruction_id,
            timing,
            enabled: true,
            destination: ExecutionDestination::FreshEachRun {
                endpoint: collaboration_protocol::EndpointRef {
                    service_id: identity()?.service_id().clone(),
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                cwd: "/workspace".into(),
            },
            execution_timeout_seconds: Some(120.try_into()?),
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
        },
    });
    match transport.call(call).await? {
        AutomationAnswer::Schedule(schedule) => Ok(schedule),
        other => Err(format!("schedule refused: {other:?}").into()),
    }
}
pub(super) async fn show(
    transport: &SessionAutomationTransport,
    firing_id: &collaboration_protocol::FiringId,
) -> ProofResult<collaboration_protocol::FiringSnapshot> {
    match transport
        .call(AutomationCall::FiringShow {
            firing_id: firing_id.clone(),
        })
        .await?
    {
        AutomationAnswer::Firing(snapshot) => Ok(snapshot),
        other => Err(format!("Firing inspection refused: {other:?}").into()),
    }
}
pub(super) fn require(condition: bool, message: &'static str) -> ProofResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
