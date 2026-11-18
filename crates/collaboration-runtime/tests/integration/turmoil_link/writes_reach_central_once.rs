//! Every write the Router accepted reaches central exactly once, through link
//! partitions, lost answers and a Router restart, and its answer names the post
//! central created the first time.
use super::{
    central_request_log::{CENTRAL_HOST, CentralLog, SharedCentralLog, serve_central},
    router_host::{
        INSTALLATION, ROUTER_HOST, RouterPlan, RouterReport, SharedRouterReport, run_router,
    },
};
use central_link_client::RequestStatus;
use central_link_protocol::ReplyPayload;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, SystemTime},
};
use workspace_model::{ItemRef, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

/// 2030-01-01T00:00:00Z: simulated UTC never comes from the machine running the test.
const EPOCH_UNIX_SECONDS: u64 = 1_893_456_000;
const TICK: Duration = Duration::from_millis(20);

/// Which way a partition drops messages.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// Nothing gets through either way: a batch may never reach central.
    BothWays,
    /// Central receives but its answers are dropped: central applies the
    /// write and the Router never hears, so the write is replayed.
    AnswersLost,
}

/// A partition between router and central, from `at` for `lasting`.
#[derive(Clone, Copy, Debug)]
struct Partition {
    at: Duration,
    lasting: Duration,
    cut: Cut,
}

struct Outcome {
    central: CentralLog,
    router: RouterReport,
}

#[expect(
    clippy::unwrap_used,
    reason = "a harness failure (tempdir, simulation) must fail the test loudly"
)]
fn run(seed: u64, plan: RouterPlan, partitions: &[Partition]) -> Outcome {
    let root = tempfile::tempdir().unwrap();
    let store_path = root.path().join("machine_local.sqlite");
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
    sim.client(ROUTER_HOST, run_router(store_path, plan, router_report));
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

fn created_post(status: &RequestStatus) -> Option<String> {
    match status {
        RequestStatus::Answered(ReplyPayload::Workspace(WorkspaceReply {
            result:
                WorkspaceResult::Done(WorkspaceAnswer::Created {
                    item: ItemRef::Post(post),
                    ..
                }),
            ..
        })) => Some(post.as_str().to_owned()),
        _ => None,
    }
}

#[expect(clippy::unwrap_used, reason = "checked by the assert just before")]
/// The invariant: each accepted write is applied at central exactly once and
/// the Router's answer names the post central created first.
fn assert_each_write_reached_central_once(outcome: &Outcome, context: &str) {
    assert!(!outcome.router.statuses.is_empty(), "{context}: no posts");
    for (id, status) in &outcome.router.statuses {
        let applied = outcome.central.applied.get(id);
        assert!(
            applied.is_some(),
            "{context}: {id} never reached central ({status:?})"
        );
        let applied = applied.unwrap();
        assert_eq!(
            applied.times_applied, 1,
            "{context}: {id} applied more than once"
        );
        assert_eq!(
            created_post(status).as_deref(),
            Some(applied.post.as_str()),
            "{context}: {id}'s answer names central's first post"
        );
    }
    for id in outcome.central.applied.keys() {
        assert!(
            outcome.router.statuses.contains_key(id),
            "{context}: central applied {id}, which the Router never accepted"
        );
    }
}

#[test]
fn writes_cross_a_partition_and_a_restart_and_reach_central_once() {
    let plan = RouterPlan {
        posts_at: vec![
            Duration::from_secs(5),
            Duration::from_secs(50),
            Duration::from_secs(70),
            Duration::from_secs(130),
            Duration::from_secs(250),
        ],
        restart_at: Some(Duration::from_secs(200)),
        answers_by: Duration::from_secs(20 * 60),
    };
    // At the 60 s mark central applies the batch but its answers are lost; at
    // the 120 s mark nothing gets through for 45 s.
    let partitions = [
        Partition {
            at: Duration::from_secs(59),
            lasting: Duration::from_secs(40),
            cut: Cut::AnswersLost,
        },
        Partition {
            at: Duration::from_secs(119),
            lasting: Duration::from_secs(45),
            cut: Cut::BothWays,
        },
    ];

    let outcome = run(7, plan, &partitions);

    assert_each_write_reached_central_once(&outcome, "fixed schedule");
    assert!(
        outcome.central.replays > 0,
        "the lost answers were replayed and central answered from its log"
    );
}

#[test]
fn seeded_fault_schedules_never_lose_or_double_a_write() {
    for seed in 0..12_u64 {
        let mut rng = SmallRng::seed_from_u64(seed);
        let posts = rng.random_range(1..=8);
        let mut posts_at: Vec<Duration> = (0..posts)
            .map(|_| Duration::from_secs(rng.random_range(1..=600)))
            .collect();
        posts_at.sort();
        let partitions: Vec<Partition> = (0..rng.random_range(0..=4))
            .map(|_| Partition {
                at: Duration::from_secs(rng.random_range(1..=600)),
                lasting: Duration::from_secs(rng.random_range(1..=120)),
                cut: if rng.random_bool(0.5) {
                    Cut::BothWays
                } else {
                    Cut::AnswersLost
                },
            })
            .collect();
        let restart_at = rng
            .random_bool(0.5)
            .then(|| Duration::from_secs(rng.random_range(1..=600)));
        let plan = RouterPlan {
            posts_at,
            restart_at,
            answers_by: Duration::from_secs(40 * 60),
        };

        let outcome = run(seed, plan.clone(), &partitions);

        assert_each_write_reached_central_once(
            &outcome,
            &format!("seed {seed}: {plan:?} partitions {partitions:?}"),
        );
    }
}
