//! Step 9 C (Lead A, 2026-11-18): every outbox store call the driver makes,
//! failed in each way it can fail, in the phase that reaches it. Each named
//! fault must fire. Then the store heals, no new work arrives, the injected
//! clock moves on, and every accepted write must be answered within a few
//! marks, by id, with central having applied it once (central's gate checks
//! every replay's caller, target and content).
use super::super::super::driver_test_support::*;
use super::super::super::scripted_central::{
    MemberFate, ScriptedCentral, created, created_post, request_id, take_batch,
};
use super::super::super::store_faults::{FaultKind, FaultPlan, FaultyOutbox};
use super::super::super::*;
use crate::outbox_store::NewOutboxItem;
use central_link_client::{LinkCalls, PendingWriteState, Submission};
use central_link_protocol::{DataFrame, LinkFrame, LinkRequest};
use runtime_primitives::Clock;
use std::time::Duration;

/// Marks a healed Router may take to answer everything it accepted.
const HEALED_WITHIN_MARKS: usize = 4;

struct Custody {
    harness: Harness,
    central: Option<CentralEnd>,
    scripted: ScriptedCentral,
    plan: FaultPlan,
    accepted: Vec<String>,
    next_seed: u16,
}

impl Custody {
    async fn start(prepare: impl AsyncFnOnce(&mut dyn OutboxStorage)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut inner = open_outbox(&root.path().join("machine_local.sqlite"))
            .await
            .unwrap();
        prepare(&mut inner).await;
        let plan = FaultPlan::default();
        let store = FaultyOutbox {
            inner: Box::new(inner),
            plan: plan.clone(),
        };
        let mut harness =
            Harness::start_with_store(root, clock_at(MARK - 20_000), Arc::new(Mutex::new(store)));
        let central = harness.connected().await;
        Self {
            harness,
            central: Some(central),
            scripted: ScriptedCentral::default(),
            plan,
            accepted: Vec::new(),
            next_seed: 0,
        }
    }

    /// Round trips through the driver's loop, so it has read what was sent
    /// and run its due timers.
    async fn settle(&self) {
        for _ in 0..3 {
            let _ = self.harness.calls.link_status().await;
        }
    }

    async fn submit(&mut self) -> String {
        let id = uuid_v7(2_000 + self.next_seed);
        self.next_seed += 1;
        let submission = self.harness.calls.submit(caller(), thread_post(&id)).await;
        self.settle().await;
        let visible =
            self.harness.local_status(&id).await != central_link_client::RequestStatus::NotFound;
        // A write is accepted when the outbox holds it, whatever the caller
        // was told: a commit whose acknowledgement was lost still flushes.
        if matches!(submission, Submission::InOutbox(_)) || visible {
            self.accepted.push(id.clone());
        }
        id
    }

    async fn next_mark(&mut self) -> Option<Vec<LinkRequest>> {
        let now = self.harness.clock.now().unix_millis();
        let next = (now.div_euclid(60_000) + 1) * 60_000;
        self.advance(u64::try_from(next - now).unwrap()).await;
        take_batch(self.central.as_mut()?)
    }

    async fn advance(&self, millis: u64) {
        self.harness
            .clock
            .advance(Duration::from_millis(millis))
            .unwrap();
        self.settle().await;
    }

    fn play_all(&mut self, members: &[LinkRequest], fate: MemberFate) {
        for member in members {
            self.scripted.play(self.central.as_ref(), member, fate);
        }
    }

    async fn drop_link(&mut self) {
        self.central = None;
        for _ in 0..64 {
            let status = self.harness.calls.link_status().await;
            if !matches!(
                status,
                Some(central_link_client::LinkStatus::Connected { .. })
            ) {
                break;
            }
        }
        self.settle().await;
    }

    async fn reconnect(&mut self) {
        for _ in 0..40 {
            self.advance(1_000).await;
            if let Ok(central) = self.harness.connections.try_recv() {
                central.welcome();
                self.central = Some(central);
                self.settle().await;
                return;
            }
        }
        panic!("the link did not reconnect within its backoff");
    }

    fn require_fired(&self, site: &'static str, kind: FaultKind) {
        assert!(
            self.plan.fired().contains(&(site, kind)),
            "{site} {kind:?} never fired: fired {:?}, still planned {:?}",
            self.plan.fired(),
            self.plan.pending()
        );
    }

    /// The store heals and no new work arrives: within a few marks every
    /// accepted write is answered with the post central created first.
    async fn heal_and_drain(&mut self, scenario: &str) {
        self.plan.clear();
        for _ in 0..HEALED_WITHIN_MARKS {
            if self.central.is_none() {
                self.reconnect().await;
                if let Some(members) = self.central.as_mut().and_then(take_batch) {
                    self.play_all(&members, MemberFate::Answered);
                    self.settle().await;
                }
            }
            if let Some(members) = self.next_mark().await {
                self.play_all(&members, MemberFate::Answered);
                self.settle().await;
            }
        }
        let pending = self.harness.calls.pending_for(caller()).await.unwrap();
        for id in &self.accepted {
            let write = pending.iter().find(|write| write.request_id.as_str() == id);
            let Some(PendingWriteState::Answered(reply)) = write.map(|write| &write.state) else {
                panic!(
                    "{scenario}: {id} not answered within {HEALED_WITHIN_MARKS} marks: {write:?}"
                );
            };
            assert_eq!(
                created_post(reply),
                self.scripted.log.get(id).map(|record| record.post.clone()),
                "{scenario}: {id} names the post central created first"
            );
        }
        assert_eq!(
            self.scripted.log.len(),
            self.accepted.len(),
            "{scenario}: central applied exactly the accepted writes"
        );
    }

    async fn stop(self) {
        self.harness.stop().await;
    }
}

/// Every faultable store call, with the phase that reaches it.
const SITES: [&str; 9] = [
    "insert",
    "earliest_waiting",
    "sent_without_answer",
    "end_overdue",
    "delete_ended_before_retention",
    "claim_next_batch",
    "record_outcome",
    "return_unapplied",
    "return_unanswered",
];

async fn reach(custody: &mut Custody, site: &'static str, kind: FaultKind) {
    match site {
        "insert" => {
            custody.plan.fail(site, kind);
            custody.submit().await;
        }
        "earliest_waiting"
        | "sent_without_answer"
        | "end_overdue"
        | "delete_ended_before_retention"
        | "claim_next_batch" => {
            custody.submit().await;
            custody.plan.fail(site, kind);
            if let Some(members) = custody.next_mark().await {
                custody.play_all(&members, MemberFate::Answered);
            }
        }
        "record_outcome" | "return_unapplied" => {
            custody.submit().await;
            let members = custody.next_mark().await.expect("the write flushes");
            custody.plan.fail(site, kind);
            let fate = if site == "record_outcome" {
                MemberFate::Answered
            } else {
                MemberFate::Unapplied
            };
            custody.play_all(&members, fate);
        }
        "return_unanswered" => {
            custody.submit().await;
            let members = custody.next_mark().await.expect("the write flushes");
            custody.play_all(&members, MemberFate::RecordedAnswerLost);
            custody.plan.fail(site, kind);
            custody.drop_link().await;
        }
        other => panic!("no phase reaches {other}"),
    }
    custody.settle().await;
}

#[tokio::test]
async fn every_store_site_failed_either_way_fires_and_the_healed_router_answers_everything() {
    for site in SITES {
        for kind in [FaultKind::BeforeCall, FaultKind::AfterCommit] {
            let mut custody = Custody::start(async |_| {}).await;
            reach(&mut custody, site, kind).await;
            custody.require_fired(site, kind);
            custody.heal_and_drain(&format!("{site} {kind:?}")).await;
            custody.stop().await;
        }
    }
}

#[tokio::test]
async fn a_lone_sent_row_at_startup_is_replayed_and_answered() {
    let id = uuid_v7(2_900);
    let mut custody = Custody::start(async |store| {
        store
            .insert(
                &NewOutboxItem {
                    request: id.parse().unwrap(),
                    caller: caller(),
                    payload: queued_thread_post(&id),
                    flush_at: SyncTime::from_unix_millis(MARK - 60_000),
                },
                SyncTime::from_unix_millis(MARK - 90_000),
            )
            .await
            .unwrap();
        store
            .mark_sent(
                &[id.parse().unwrap()],
                SyncTime::from_unix_millis(MARK - 60_000),
            )
            .await
            .unwrap();
    })
    .await;
    custody.accepted.push(id.clone());

    custody.heal_and_drain("lone sent row").await;

    assert_eq!(custody.scripted.received.get(&id), Some(&1));
    custody.stop().await;
}

/// A batch whose answers never come times out on a healthy socket; its write
/// is replayed on the next mark with the same caller and payload and a fresh
/// correlation. The old reply, released late, cannot settle the replay.
#[tokio::test]
async fn a_timed_out_batch_is_replayed_and_its_late_reply_cannot_settle_the_replay() {
    for fault in [
        None,
        Some(FaultKind::BeforeCall),
        Some(FaultKind::AfterCommit),
    ] {
        let mut custody = Custody::start(async |_| {}).await;
        let id = custody.submit().await;
        let first = custody.next_mark().await.expect("the write flushes");
        if let Some(kind) = fault {
            custody.plan.fail("return_unanswered", kind);
        }
        // No answer: the call deadline (30 s) passes with the socket healthy.
        custody.advance(31_000).await;
        if let Some(kind) = fault {
            custody.require_fired("return_unanswered", kind);
        }

        let mut replay = None;
        for _ in 0..HEALED_WITHIN_MARKS {
            replay = custody.next_mark().await;
            if replay.is_some() {
                break;
            }
        }
        let replay = replay.unwrap_or_else(|| panic!("{fault:?}: no replay within the marks"));
        assert_eq!(replay.len(), 1);
        assert_eq!(request_id(&replay[0]), id);
        assert_eq!(replay[0].caller(), first[0].caller());
        assert_eq!(replay[0].payload(), first[0].payload());
        assert_ne!(
            replay[0].correlation_id(),
            first[0].correlation_id(),
            "the replay is a fresh call"
        );
        let central = custody.central.as_ref().unwrap();
        central.send(LinkFrame::Data(DataFrame::Reply {
            correlation_id: first[0].correlation_id().clone(),
            result: Box::new(created(&uuid_v7(4_999), 999)),
        }));
        custody.settle().await;
        let after_late_reply = custody
            .harness
            .calls
            .request_status(caller(), id.parse().unwrap())
            .await;
        assert_eq!(
            after_late_reply,
            central_link_client::RequestStatus::Sent,
            "{fault:?}: the old correlation's reply does not settle the replay"
        );
        custody.play_all(&replay, MemberFate::Answered);
        custody.settle().await;

        custody
            .heal_and_drain(&format!("timed out {fault:?}"))
            .await;
        custody.stop().await;
    }
}

async fn pending_state(custody: &Custody, id: &str) -> PendingWriteState {
    custody
        .harness
        .calls
        .pending_for(caller())
        .await
        .unwrap()
        .into_iter()
        .find(|write| write.request_id.as_str() == id)
        .map(|write| write.state)
        .expect("the write is pending")
}

/// The shrunk Hegel case (step 9 C): central applied the first attempt and
/// the answer was lost; link admission then refused the replay. The write
/// may be in central, so it must not read as unsent.
#[tokio::test]
async fn a_replay_that_was_not_applied_still_reads_as_possibly_in_central() {
    let mut custody = Custody::start(async |_| {}).await;
    let id = custody.submit().await;
    let first = custody.next_mark().await.expect("the write flushes");
    custody.play_all(&first, MemberFate::RecordedAnswerLost);
    custody.drop_link().await;
    custody.reconnect().await;
    let mut replay = custody.central.as_mut().and_then(take_batch);
    for _ in 0..HEALED_WITHIN_MARKS {
        if replay.is_some() {
            break;
        }
        replay = custody.next_mark().await;
    }
    let replay = replay.expect("the write is replayed within the marks");
    assert_eq!(request_id(&replay[0]), id);

    custody.play_all(&replay, MemberFate::Unapplied);
    custody.settle().await;

    assert!(
        matches!(
            pending_state(&custody, &id).await,
            PendingWriteState::SentAnswerLost { .. }
        ),
        "central holds it from the first attempt"
    );
    assert!(custody.scripted.log.contains_key(&id));
    custody.heal_and_drain("unapplied replay").await;
    custody.stop().await;
}

/// A first attempt that link admission refused never reached central: it
/// reads as unsent, exactly as before.
#[tokio::test]
async fn a_first_attempt_that_was_not_applied_reads_as_unsent() {
    let mut custody = Custody::start(async |_| {}).await;
    let id = custody.submit().await;
    let first = custody.next_mark().await.expect("the write flushes");

    custody.play_all(&first, MemberFate::Unapplied);
    custody.settle().await;

    assert!(matches!(
        pending_state(&custody, &id).await,
        PendingWriteState::Unsent { .. }
    ));
    assert!(!custody.scripted.log.contains_key(&id));
    custody.heal_and_drain("unapplied first attempt").await;
    custody.stop().await;
}
