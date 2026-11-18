//! The overlay invariant (Lead A, option C) against the real driver, store and
//! flush, with a scripted central that keeps a request log like central's.
//! Each flush member is answered, recorded with its answer lost, or lost
//! before central records it; links drop and reconnect. After every step the
//! caller's own view (central's records plus `pending_for`) shows:
//! - an unsent write once, and central does not have it;
//! - an answered write once, with the id central created first;
//! - a sent write whose answer was lost at most twice (pending plus central)
//!   until the next flush's replay answers it, then once.
use super::driver_test_support::*;
use central_link_client::{LinkCalls, PendingWrite, PendingWriteState, Submission};
use central_link_protocol::{DataFrame, FrameDirection, LinkFrame, ReplyPayload, decode_text};
use hegel::{TestCase, generators as gs};
use runtime_primitives::Clock;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use workspace_model::{ChangePosition, ItemRef, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

/// What central does with one batch member.
#[derive(Clone, Copy, Debug)]
enum MemberFate {
    Answered,
    RecordedAnswerLost,
    LostBeforeCentral,
}

struct OverlayModel {
    runtime: tokio::runtime::Runtime,
    harness: Harness,
    central: Option<CentralEnd>,
    /// Central's request log: request id -> the post it created, once.
    central_log: BTreeMap<String, String>,
    submitted: BTreeSet<String>,
    next_seed: u16,
    next_post: u16,
}

fn created(post: &str, position: u64) -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Done(WorkspaceAnswer::Created {
            item: ItemRef::Post(post.parse().unwrap()),
            position: ChangePosition::try_from(position).unwrap(),
        }),
        actors: Vec::new(),
    })
}

fn created_post(reply: &ReplyPayload) -> Option<String> {
    match reply {
        ReplyPayload::Workspace(WorkspaceReply {
            result:
                WorkspaceResult::Done(WorkspaceAnswer::Created {
                    item: ItemRef::Post(post),
                    ..
                }),
            ..
        }) => Some(post.as_str().to_owned()),
        _ => None,
    }
}

impl OverlayModel {
    /// Two command round trips: the first may be served before the driver
    /// looks at the clock; by the second it has run its due timers.
    fn settle(&self) {
        for _ in 0..2 {
            let _ = self.runtime.block_on(self.harness.calls.link_status());
        }
    }

    fn advance(&self, millis: u64) {
        self.harness
            .clock
            .advance(Duration::from_millis(millis))
            .unwrap();
        self.settle();
    }

    fn pending(&self) -> Vec<PendingWrite> {
        self.runtime
            .block_on(self.harness.calls.pending_for(caller()))
            .unwrap()
    }

    /// Answers heartbeats and returns the batch frame's members, if one was sent.
    fn take_batch(&mut self) -> Option<Vec<central_link_protocol::LinkRequest>> {
        let central = self.central.as_mut()?;
        let mut batch = None;
        while let Ok(text) = central.from_router.try_recv() {
            match decode_text(&text, FrameDirection::ToCentral).unwrap() {
                LinkFrame::Ping => central.send(LinkFrame::Pong),
                LinkFrame::Data(DataFrame::Batch { requests }) => {
                    assert!(batch.is_none(), "one batch per flush");
                    batch = Some(requests.requests().to_vec());
                }
                other => panic!("unexpected frame from the Router: {other:?}"),
            }
        }
        batch
    }

    /// Plays central for a batch in flight; returns whether every member was
    /// answered. The view is read only after the batch ended.
    fn resolve_batch(&mut self, tc: &TestCase) -> Option<(Vec<String>, bool)> {
        let members = self.take_batch()?;
        let mut ids = Vec::new();
        let mut all_answered = true;
        for member in &members {
            let Some(request) = member.payload().write_request_id() else {
                panic!("a batch member is an outbox write with its id");
            };
            let id = request.as_str().to_owned();
            assert!(
                self.submitted.contains(&id),
                "only submitted writes are sent"
            );
            ids.push(id.clone());
            let fate = tc.draw(gs::sampled_from(vec![
                MemberFate::Answered,
                MemberFate::RecordedAnswerLost,
                MemberFate::LostBeforeCentral,
            ]));
            if !matches!(fate, MemberFate::LostBeforeCentral) {
                // Central's 24 h request log: a replay returns the first post.
                let next_post = &mut self.next_post;
                self.central_log.entry(id.clone()).or_insert_with(|| {
                    *next_post += 1;
                    uuid_v7(40_000 + *next_post)
                });
            }
            if matches!(fate, MemberFate::Answered) {
                let post = self.central_log[&id].clone();
                let position = u64::from(self.next_post);
                if let Some(central) = &self.central {
                    central.send(LinkFrame::Data(DataFrame::Reply {
                        correlation_id: member.correlation_id().clone(),
                        result: Box::new(created(&post, position)),
                    }));
                }
            } else {
                all_answered = false;
            }
        }
        if !all_answered {
            // The rest of the batch's answers never arrive: the link drops.
            self.central = None;
        }
        // pending_for waits until the batch in flight has ended.
        let _ = self.pending();
        Some((ids, all_answered))
    }
}

#[hegel::state_machine]
impl OverlayModel {
    #[rule(weight = 3)]
    fn a_session_posts(&mut self, tc: TestCase) {
        let id = uuid_v7(1_000 + self.next_seed);
        self.next_seed += 1;
        let submission = self
            .runtime
            .block_on(self.harness.calls.submit(caller(), thread_post(&id)));
        assert!(
            matches!(submission, Submission::InOutbox(_)),
            "{submission:?}"
        );
        self.submitted.insert(id);
        // A write stored inside a mark's window that passed while the link was
        // down can start a batch at once; central answers it in this step.
        self.settle();
        let _ = self.resolve_batch(&tc);
    }

    #[rule(weight = 3)]
    fn the_next_mark_flushes(&mut self, tc: TestCase) {
        tc.assume(self.central.is_some());
        let now = self.harness.clock.now().unix_millis();
        let next_mark = (now.div_euclid(60_000) + 1) * 60_000;
        self.advance(u64::try_from(next_mark - now).unwrap());
        if let Some((members, true)) = self.resolve_batch(&tc) {
            // The replay resolved every lost answer: each member shows once.
            let pending = self.pending();
            for id in members {
                let write = pending.iter().find(|write| write.request_id.as_str() == id);
                assert!(
                    matches!(
                        write.map(|write| &write.state),
                        Some(PendingWriteState::Answered(_))
                    ),
                    "an answered member is Answered: {write:?}"
                );
            }
        }
    }

    #[rule]
    fn the_link_drops(&mut self, tc: TestCase) {
        tc.assume(self.central.is_some());
        self.central = None;
        self.settle();
    }

    #[rule(weight = 2)]
    fn the_link_reconnects(&mut self, tc: TestCase) {
        tc.assume(self.central.is_none());
        // The client's backoff runs on the injected clock (at most 30 s).
        for _ in 0..40 {
            self.advance(1_000);
            if let Ok(central) = self.harness.connections.try_recv() {
                central.welcome();
                self.central = Some(central);
                self.settle();
                let _ = self.resolve_batch(&tc);
                return;
            }
        }
        panic!("the link did not reconnect within its backoff");
    }

    #[invariant(always_run)]
    fn each_own_write_shows_as_option_c_allows(&self, _: TestCase) {
        let pending = self.pending();
        let mut seen = BTreeSet::new();
        for write in &pending {
            let id = write.request_id.as_str().to_owned();
            assert!(seen.insert(id.clone()), "{id} is pending twice");
            assert!(self.submitted.contains(&id), "{id} was never submitted");
            let in_central = self.central_log.get(&id);
            match &write.state {
                PendingWriteState::Unsent { .. } => {
                    assert!(in_central.is_none(), "an unsent write is not in central");
                }
                // Shown by pending and possibly by central: at most twice.
                PendingWriteState::SentAnswerLost { .. } => {}
                PendingWriteState::Answered(reply) => {
                    assert_eq!(
                        created_post(reply).as_ref(),
                        in_central,
                        "the answer names the post central created first"
                    );
                }
            }
        }
        assert_eq!(seen, self.submitted, "every write is in the caller's view");
    }
}

#[hegel::test(test_cases = 40)]
fn each_write_shows_once_except_a_lost_answer_until_its_replay(tc: TestCase) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut harness = runtime.block_on(Harness::start(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000),
    ));
    let central = runtime.block_on(harness.connected());
    let model = OverlayModel {
        runtime,
        harness,
        central: Some(central),
        central_log: BTreeMap::new(),
        submitted: BTreeSet::new(),
        next_seed: 0,
        next_post: 0,
    };
    hegel::stateful::machine(model).run(tc);
}
