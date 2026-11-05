//! Preserve thread relationships, participation, watches and read marks in the L2 source.
use super::{
    offline_cli_fixture::TestResult,
    recovery_rows::require,
    rich_source_records::{RichRecords, accepted},
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::ThreadSubscribeRequest;
use message_board::{
    EpochNumber, FeedCursor, FeedPosition, FeedReadRequest, PageLimit, ParticipantRole, PostId,
    PostTarget, ProjectWatchRequest, ProtocolVersion, ReadActor, ReadMarkOutcome, ReferenceTarget,
    RequestId, SubscriptionMode, SubscriptionPolicyPatch, SubscriptionTarget, ThreadCreateRequest,
    ThreadPostRequest, ThreadRef, ThreadRefileRequest, ThreadShowRequest, ThreadSubject,
};

pub(super) const ROOT_POST: &str = "source root post: \"verbatim\" 🐒";
pub(super) const REPLY_POST: &str = "source reply\nwith an intact reference";
pub(super) struct RichMessages {
    pub thread: ThreadRef,
    pub root_post: PostId,
    pub reply: PostId,
}

pub(super) async fn seed(
    client: &CollaborationClient,
    records: &RichRecords,
) -> TestResult<RichMessages> {
    let root_post = PostId::generate();
    let reply = PostId::generate();
    accepted(
        client
            .thread_create(ThreadCreateRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: root_post.clone(),
                project_id: records.project.clone(),
                about: ThreadSubject::Task(records.first_task.clone()),
                actor: records.actor.clone(),
                acting_for: None,
                text: ROOT_POST.to_owned().try_into()?,
                references: vec![].try_into()?,
                addressees: vec![],
                role: Some(ParticipantRole::Participant),
                watch: true,
            })
            .await?,
        "source thread",
    )?;
    let thread = ThreadRef {
        thread: root_post.clone().into(),
        project: Some(records.project.clone()),
    };
    accepted(
        client
            .thread_post(ThreadPostRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                post_id: reply.clone(),
                placement: PostTarget::Thread {
                    thread: thread.clone(),
                },
                actor: records.actor.clone(),
                acting_for: None,
                text: REPLY_POST.to_owned().try_into()?,
                references: vec![ReferenceTarget::Post {
                    post_id: root_post.clone(),
                }]
                .try_into()?,
                addressees: vec![],
            })
            .await?,
        "source reply",
    )?;
    accepted(
        client
            .thread_refile(ThreadRefileRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                thread: thread.clone(),
                actor: records.actor.clone(),
                acting_for: None,
                about: ThreadSubject::Workstream(records.workstream.clone()),
            })
            .await?,
        "source thread refile",
    )?;
    accepted(
        client
            .project_watch(ProjectWatchRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: records.project.clone(),
                actor: records.actor.clone(),
                acting_for: None,
            })
            .await?,
        "source project watch",
    )?;
    accepted(
        client
            .thread_subscribe(ThreadSubscribeRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                acting_for: None,
                actor: records.actor.clone(),
                scope: SubscriptionTarget::Thread {
                    thread: thread.clone(),
                },
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
                    ..SubscriptionPolicyPatch::default()
                },
            })
            .await?,
        "source explicit poll membership",
    )?;
    let shown = client
        .thread_show(ThreadShowRequest {
            thread: thread.clone(),
            reader: ReadActor::Identified {
                reader: records.actor.clone(),
            },
            include_redacted: true,
        })
        .await?;
    let covered = client
        .feed_read(FeedReadRequest {
            reader: ReadActor::Identified {
                reader: records.actor.clone(),
            },
            project_id: records.project.clone(),
            after: FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::ZERO,
            },
            limit: PageLimit::try_from(100)?,
            include_redacted: true,
        })
        .await?;
    require(
        matches!(covered.read_mark, Some(ReadMarkOutcome::Advanced { .. })),
        "covered source feed read did not persist its mark",
    )?;
    require(
        shown
            .watch_status
            .as_ref()
            .is_some_and(|status| status.watching),
        "source watch missing",
    )?;
    Ok(RichMessages {
        thread,
        root_post,
        reply,
    })
}
