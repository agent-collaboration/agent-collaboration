//! Every write the Router accepted reaches central exactly once, through link
//! partitions, lost answers and a Router restart, and its answer names the post
//! central created the first time.
use central_link_client::RequestStatus;
use central_link_protocol::ReplyPayload;
use collaboration_link_test_support::{
    router_host::RouterPlan,
    write_simulation::{Cut, Outcome, Partition, run_write_simulation},
};
use rand::{Rng, SeedableRng, rngs::SmallRng};
use std::time::Duration;
use workspace_model::{ItemRef, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

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

    let outcome = run_write_simulation(7, plan, &partitions);

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

        let outcome = run_write_simulation(seed, plan.clone(), &partitions);

        assert_each_write_reached_central_once(
            &outcome,
            &format!("seed {seed}: {plan:?} partitions {partitions:?}"),
        );
    }
}

#[test]
fn the_same_seed_gives_the_same_timeline_twice() {
    let plan = RouterPlan {
        posts_at: vec![
            Duration::from_secs(5),
            Duration::from_secs(50),
            Duration::from_secs(130),
        ],
        restart_at: Some(Duration::from_secs(100)),
        answers_by: Duration::from_secs(20 * 60),
    };
    let partitions = [Partition {
        at: Duration::from_secs(59),
        lasting: Duration::from_secs(40),
        cut: Cut::AnswersLost,
    }];

    let first = run_write_simulation(11, plan.clone(), &partitions);
    let second = run_write_simulation(11, plan, &partitions);

    assert!(!first.central.timeline.is_empty());
    assert_eq!(first.central.timeline, second.central.timeline);
    assert_eq!(first.router.statuses, second.router.statuses);
}
