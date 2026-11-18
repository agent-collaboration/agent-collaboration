//! Simulated host "router": the production RouterLinkCalls over the real Mac
//! outbox store, submitting thread posts on a schedule, optionally restarting
//! its driver part-way, then waiting for every write's answer.
use super::{
    central_request_log::{CENTRAL_HOST, LINK_PORT},
    simulated_clock::SimulatedClock,
    turmoil_transport::TurmoilLinkTransport,
};
use central_link_client::{FlushOutcomeSink, FlushOutcomes, LinkCalls, RequestStatus, Submission};
use central_link_protocol::{RequestPayload, TokenRenewalResult};
use collaboration_runtime::{
    CentralLinkParts, FlushJitter, MachineTokenSource, MemoryOutbox, OutboxStorage,
    RouterLinkCalls, TokenAvailability, TransportFuture,
};
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::{Caller, MachineId, SessionAddress, WorkspaceOperation, WorkspaceRequest};

pub const ROUTER_HOST: &str = "router";
pub const INSTALLATION: &str = "01890f2e-7b4c-7cc0-98c4-000000000002";

/// When the router posts, restarts, and how long it waits for the answers.
#[derive(Clone, Debug)]
pub struct RouterPlan {
    pub posts_at: Vec<Duration>,
    pub restart_at: Option<Duration>,
    pub answers_by: Duration,
}

/// Each posted request id and the last status the router read for it.
#[derive(Default)]
pub struct RouterReport {
    pub statuses: BTreeMap<String, RequestStatus>,
}

pub type SharedRouterReport = Rc<RefCell<RouterReport>>;

struct ReadyToken;
impl MachineTokenSource for ReadyToken {
    fn availability(&self) -> TokenAvailability {
        #[expect(clippy::unwrap_used, reason = "a fixed, valid fixture token")]
        TokenAvailability::Ready("simulated.machine.token".to_owned().try_into().unwrap())
    }
    fn changed(&self) -> TransportFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
    fn report_renewal(&self, _outcome: &TokenRenewalResult) {}
}

struct OnTheMark;
impl FlushJitter for OnTheMark {
    fn jitter_millis(&self, _mark: SyncTime) -> i64 {
        0
    }
}

struct IgnoredOutcomes;
impl FlushOutcomeSink for IgnoredOutcomes {
    fn deliver(&self, _outcomes: FlushOutcomes) {}
    fn ended(&self, _ended: Vec<central_link_client::EndedWrite>) {}
}

fn installation() -> MachineId {
    #[expect(clippy::unwrap_used, reason = "a fixed UUIDv7")]
    INSTALLATION.parse().unwrap()
}

pub fn caller() -> Caller {
    #[expect(clippy::unwrap_used, reason = "fixed valid session fields")]
    Caller::Session(SessionAddress {
        machine_id: installation(),
        endpoint: "codex-local".to_owned().try_into().unwrap(),
        session_id: "simulated-session".to_owned().try_into().unwrap(),
    })
}

/// The `index`th post's request id, a UUIDv7.
pub fn post_id(index: usize) -> String {
    format!("01890f2e-7b4c-7cc0-98c5-{index:012x}")
}

fn thread_post(request_id: &str) -> RequestPayload {
    #[expect(clippy::unwrap_used, reason = "fixed valid ids and text")]
    RequestPayload::Workspace(WorkspaceRequest {
        request_id: Some(request_id.parse().unwrap()),
        workspace_id: Some("01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap()),
        operation: WorkspaceOperation::ThreadPost {
            thread_id: "01890f2e-7b4c-7cc0-98c4-000000000005".parse().unwrap(),
            body: "simulated post".to_owned().try_into().unwrap(),
            addressees: Vec::new(),
        },
    })
}

struct RunningLink {
    calls: RouterLinkCalls,
    tasks: TaskTracker,
    stop: CancellationToken,
}

fn start_link(clock: Arc<dyn RuntimeClock>, store: Arc<Mutex<dyn OutboxStorage>>) -> RunningLink {
    let tasks = TaskTracker::new();
    let stop = CancellationToken::new();
    let calls = RouterLinkCalls::start(
        CentralLinkParts {
            installation: installation(),
            clock,
            store,
            transport: Arc::new(TurmoilLinkTransport::new(CENTRAL_HOST, LINK_PORT, "/link")),
            tokens: Arc::new(ReadyToken),
            jitter: Arc::new(OnTheMark),
            outcomes: Arc::new(IgnoredOutcomes),
        },
        &tasks,
        stop.clone(),
    );
    RunningLink { calls, tasks, stop }
}

impl RunningLink {
    async fn stop(self) {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

/// Host software for "router".
pub async fn run_router(plan: RouterPlan, report: SharedRouterReport) -> turmoil::Result {
    let clock: Arc<dyn RuntimeClock> = Arc::new(SimulatedClock::for_host(ROUTER_HOST));
    // In memory, so a store call never leaves the simulated host (Lead A's
    // call): sqlx-sqlite's worker thread turned real I/O latency into
    // simulated time. The shared contract suite holds it to the real store.
    let store: Arc<Mutex<dyn OutboxStorage>> = Arc::new(Mutex::new(MemoryOutbox::new()));
    let mut link = start_link(clock.clone(), store.clone());
    let mut restarted = false;
    let started = clock.monotonic_now();
    for (index, at) in plan.posts_at.iter().enumerate() {
        if let Some(restart_at) = plan.restart_at
            && !restarted
            && restart_at <= *at
        {
            clock.sleep_until(started.after(restart_at)).await;
            // A Router restart: the driver stops; a new one restores the outbox.
            link.stop().await;
            link = start_link(clock.clone(), store.clone());
            restarted = true;
        }
        clock.sleep_until(started.after(*at)).await;
        let id = post_id(index);
        let submission = link.calls.submit(caller(), thread_post(&id)).await;
        if !matches!(submission, Submission::InOutbox(_)) {
            return Err(format!("post {id} was not stored in the outbox: {submission:?}").into());
        }
        report
            .borrow_mut()
            .statuses
            .insert(id, RequestStatus::NotFound);
    }
    let deadline = started.after(plan.answers_by);
    loop {
        let mut all_answered = true;
        let ids: Vec<String> = report.borrow().statuses.keys().cloned().collect();
        for id in ids {
            #[expect(clippy::unwrap_used, reason = "ids minted as UUIDv7 above")]
            let status = link
                .calls
                .request_status(caller(), id.parse().unwrap())
                .await;
            all_answered &= matches!(status, RequestStatus::Answered(_));
            report.borrow_mut().statuses.insert(id, status);
        }
        if all_answered || clock.monotonic_now() >= deadline {
            break;
        }
        clock
            .sleep_until(clock.monotonic_now().after(Duration::from_secs(5)))
            .await;
    }
    link.stop().await;
    Ok(())
}
