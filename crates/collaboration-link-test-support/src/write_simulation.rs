//! Runs the production Router link against a simulated central request log.
use crate::{
    central_request_log::{CENTRAL_HOST, CentralLog, SharedCentralLog, serve_central},
    router_host::{
        INSTALLATION, ROUTER_HOST, RouterPlan, RouterReport, SharedRouterReport, run_router,
    },
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, SystemTime},
};

/// 2030-01-01T00:00:00Z: simulated UTC never comes from the machine running the test.
const EPOCH_UNIX_SECONDS: u64 = 1_893_456_000;
const TICK: Duration = Duration::from_millis(20);

/// Which way a partition drops messages.
#[derive(Clone, Copy, Debug)]
pub enum Cut {
    /// Nothing gets through either way: a batch may never reach central.
    BothWays,
    /// Central receives but its answers are dropped: central applies the
    /// write and the Router never hears, so the write is replayed.
    AnswersLost,
}

/// A partition between router and central, from `at` for `lasting`.
#[derive(Clone, Copy, Debug)]
pub struct Partition {
    pub at: Duration,
    pub lasting: Duration,
    pub cut: Cut,
}

pub struct Outcome {
    pub central: CentralLog,
    pub router: RouterReport,
}

#[expect(
    clippy::unwrap_used,
    reason = "a simulation harness failure must fail the test loudly"
)]
pub fn run_write_simulation(seed: u64, plan: RouterPlan, partitions: &[Partition]) -> Outcome {
    let central_log: SharedCentralLog = Rc::new(RefCell::new(CentralLog::default()));
    let report: SharedRouterReport = Rc::new(RefCell::new(RouterReport::default()));
    let mut sim = turmoil::Builder::new()
        .rng_seed(seed)
        .epoch(SystemTime::UNIX_EPOCH + Duration::from_secs(EPOCH_UNIX_SECONDS))
        .tick_duration(TICK)
        .simulation_duration(Duration::from_secs(3 * 60 * 60))
        .min_message_latency(Duration::from_millis(1))
        .max_message_latency(Duration::from_millis(80))
        .build();
    let central = Rc::clone(&central_log);
    sim.host(CENTRAL_HOST, move || {
        serve_central(INSTALLATION.parse().unwrap(), Rc::clone(&central))
    });
    let router_report = Rc::clone(&report);
    sim.client(ROUTER_HOST, run_router(plan, router_report));
    let mut partitioned_until: Option<(Duration, Cut)> = None;
    let mut pending: Vec<Partition> = partitions.to_vec();
    pending.sort_by_key(|partition| partition.at);
    loop {
        let now = sim.elapsed();
        if let Some((until, cut)) = partitioned_until
            && now >= until
        {
            match cut {
                Cut::BothWays => sim.repair(ROUTER_HOST, CENTRAL_HOST),
                Cut::AnswersLost => sim.repair_oneway(CENTRAL_HOST, ROUTER_HOST),
            }
            partitioned_until = None;
        }
        if partitioned_until.is_none()
            && let Some(next) = pending.first().copied()
            && now >= next.at
        {
            pending.remove(0);
            match next.cut {
                Cut::BothWays => sim.partition(ROUTER_HOST, CENTRAL_HOST),
                Cut::AnswersLost => sim.partition_oneway(CENTRAL_HOST, ROUTER_HOST),
            }
            partitioned_until = Some((now + next.lasting, next.cut));
        }
        if sim.step().unwrap() {
            break;
        }
    }
    drop(sim);
    Outcome {
        central: Rc::try_unwrap(central_log).ok().unwrap().into_inner(),
        router: Rc::try_unwrap(report).ok().unwrap().into_inner(),
    }
}
