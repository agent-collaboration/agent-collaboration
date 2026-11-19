//! A scripted central for the outbox custody tests: it keeps a request log
//! like central's and checks every replay the way central's gate does, by
//! caller and by the content digest over target and operation. A replay that
//! differs is a Router defect, so it panics instead of answering.
use super::driver_test_support::CentralEnd;
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, ReplyPayload, RequestPayload, decode_text,
};
use std::collections::BTreeMap;
use workspace_model::{
    Caller, ChangePosition, ItemRef, RefusalReason, WorkspaceAnswer, WorkspaceReply,
    WorkspaceResult, operation_content_digest,
};

/// What central does with one batch member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemberFate {
    /// Applied once and answered.
    Answered,
    /// Applied, but the answer never reaches the Router.
    RecordedAnswerLost,
    /// Lost on the way: central never sees it.
    LostBeforeCentral,
    /// Link admission said the machine was revoked: not applied, answered.
    Unapplied,
}

#[derive(Clone, Debug)]
pub(crate) struct CentralRecord {
    pub caller: Caller,
    pub digest: String,
    pub post: String,
}

#[derive(Default)]
pub(crate) struct ScriptedCentral {
    /// Request id to the decision central recorded first.
    pub log: BTreeMap<String, CentralRecord>,
    /// How many times central received each request id.
    pub received: BTreeMap<String, usize>,
    next_post: u16,
}

pub(crate) fn created(post: &str, position: u64) -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Done(WorkspaceAnswer::Created {
            item: ItemRef::Post(post.parse().unwrap()),
            position: ChangePosition::try_from(position).unwrap(),
        }),
        actors: Vec::new(),
    })
}

fn unapplied() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Refused(RefusalReason::MachineRevoked),
        actors: Vec::new(),
    })
}

pub(crate) fn created_post(reply: &ReplyPayload) -> Option<String> {
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

/// Answers heartbeats and returns the batch frame's members, if one was sent.
pub(crate) fn take_batch(central: &mut CentralEnd) -> Option<Vec<LinkRequest>> {
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

pub(crate) fn request_id(member: &LinkRequest) -> String {
    member
        .payload()
        .write_request_id()
        .expect("a batch member is an outbox write with its id")
        .as_str()
        .to_owned()
}

impl ScriptedCentral {
    /// Central's gate: the first arrival is recorded; a replay must carry the
    /// same caller and the same target and content, and gets the first post.
    fn admit(&mut self, member: &LinkRequest) -> String {
        let id = request_id(member);
        let digest = match member.payload() {
            RequestPayload::Workspace(request) => operation_content_digest(&request.operation),
            RequestPayload::Mail(request) => operation_content_digest(&request.operation),
        }
        .unwrap();
        if let Some(record) = self.log.get(&id) {
            assert_eq!(
                &record.caller,
                member.caller(),
                "{id}: replay changed caller"
            );
            assert_eq!(
                record.digest, digest,
                "{id}: replay changed target or content"
            );
            return record.post.clone();
        }
        self.next_post += 1;
        let post = super::driver_test_support::uuid_v7(40_000 + self.next_post);
        self.log.insert(
            id,
            CentralRecord {
                caller: member.caller().clone(),
                digest,
                post: post.clone(),
            },
        );
        post
    }

    /// Plays one member's fate; returns whether an answer was sent.
    pub(crate) fn play(
        &mut self,
        central: Option<&CentralEnd>,
        member: &LinkRequest,
        fate: MemberFate,
    ) -> bool {
        if fate != MemberFate::LostBeforeCentral {
            *self.received.entry(request_id(member)).or_default() += 1;
        }
        let reply = match fate {
            MemberFate::LostBeforeCentral => return false,
            MemberFate::Unapplied => unapplied(),
            MemberFate::RecordedAnswerLost => {
                self.admit(member);
                return false;
            }
            MemberFate::Answered => {
                let post = self.admit(member);
                created(&post, u64::from(self.next_post))
            }
        };
        if let Some(central) = central {
            central.send(LinkFrame::Data(DataFrame::Reply {
                correlation_id: member.correlation_id().clone(),
                result: Box::new(reply),
            }));
        }
        true
    }
}
