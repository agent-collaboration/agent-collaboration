use super::{
    OWNER_HUMAN_ID, PRIMARY_BOARD, PRIMARY_PROJECT, PRIMARY_TOPIC, REPOSITORY_ORIGIN,
    SECONDARY_PROJECT,
};
use crate::proof_context::{ProofContext, ProofResult};
use collaboration_client::board::*;

pub(super) struct NamedResources {
    pub primary_workspace: Workspace,
    pub primary_board: Board,
    pub primary_project: ProjectDirectoryEntry,
}

pub(super) struct DiscussionResources {
    pub primary_root: PostId,
    pub cross_project_reply: PostId,
}

pub(super) async fn discover_named_resources(
    proof: &mut ProofContext,
) -> ProofResult<NamedResources> {
    let projects = proof
        .client
        .workspace_list(WorkspaceListRequest {
            repository: Some(RepositoryRef::Origin {
                normalized_origin: NormalizedOrigin::try_from(REPOSITORY_ORIGIN.to_owned())?,
            }),
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    let primary_workspace = projects
        .into_iter()
        .find(|project| project.name.as_str() == PRIMARY_PROJECT)
        .ok_or("setup operator did not create/discover the authorized primary project")?;
    let boards = proof
        .client
        .board_list(BoardListRequest {
            workspace_id: Some(primary_workspace.workspace_id.clone()),
            include_archived: true,
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    let primary_board = boards
        .into_iter()
        .find(|board| board.name.as_str() == PRIMARY_BOARD)
        .ok_or("setup operator did not create the authorized primary board")?;
    let topics = proof
        .client
        .project_list(ProjectListRequest {
            board_id: primary_board.board_id.clone(),
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    let primary_project = topics
        .into_iter()
        .find(|topic| topic.title.as_str() == PRIMARY_TOPIC)
        .ok_or("setup operator did not create the authorized primary topic")?;
    Ok(NamedResources {
        primary_workspace,
        primary_board,
        primary_project,
    })
}

pub(super) async fn grade_discussion(
    proof: &mut ProofContext,
    resources: &NamedResources,
    discussion_identity: &Identity,
) -> ProofResult<DiscussionResources> {
    let primary_messages = proof
        .client
        .post_list(PostListRequest {
            scope: PostListScope::Project {
                project_id: resources.primary_project.id.clone(),
            },
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    let primary_root = primary_messages
        .into_iter()
        .find(|message| {
            message.actor == *discussion_identity
                && message.text.as_str().contains("Migration readiness")
        })
        .ok_or("discussion operator did not create the primary Migration readiness root")?;
    let projects = proof
        .client
        .workspace_list(WorkspaceListRequest {
            repository: None,
            page: PageRequest {
                limit: PageLimit::try_from(100)?,
                cursor: None,
            },
        })
        .await?
        .page
        .records;
    let secondary = projects
        .into_iter()
        .find(|project| project.name.as_str() == SECONDARY_PROJECT)
        .ok_or("discussion operator did not create the authorized secondary project")?;
    let secondary_board = proof
        .client
        .board_list(BoardListRequest {
            workspace_id: Some(secondary.workspace_id),
            include_archived: true,
            page: PageRequest::default(),
        })
        .await?
        .page
        .records
        .into_iter()
        .next()
        .ok_or("secondary project has no board")?;
    let secondary_messages = proof
        .client
        .post_list(PostListRequest {
            scope: PostListScope::Board {
                board_id: secondary_board.board_id,
            },
            selection: PostSelection::Latest,
            page: PageRequest {
                limit: PageLimit::try_from(100)?,
                cursor: None,
            },
        })
        .await?
        .page
        .records;
    let owner = ActingForIdentity::Human {
        human_id: HumanId::try_from(OWNER_HUMAN_ID.to_owned())?,
    };
    let reply = secondary_messages.into_iter().find(|message| {
        message.actor == *discussion_identity
            && message.acting_for == Some(owner.clone())
            && matches!(message.placement, Placement::Thread { .. })
            && message.text.as_str().contains("42")
            && message.references.iter().any(|reference| reference == &ReferenceTarget::Post { post_id: primary_root.post_id.clone() })
            && message.references.iter().any(|reference| reference == &ReferenceTarget::Thread { thread_id: primary_root.post_id.clone().into() })
    }).ok_or("cross-project reply had wrong actor, acting-for, placement, references, or fixture result")?;
    let watch = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (reply.placement.clone().thread_root()?).into(),
            reader: ReadActor::Identified {
                reader: discussion_identity.clone(),
            },
        })
        .await?;
    if watch.watch_status.is_none_or(|status| !status.watching) {
        return Err(
            "discussion operator did not retain the explicit Watch created with its Thread".into(),
        );
    }
    Ok(DiscussionResources {
        primary_root: primary_root.post_id,
        cross_project_reply: reply.post_id,
    })
}

pub(super) async fn grade_inbox(
    proof: &mut ProofContext,
    resources: &NamedResources,
    discussions: &DiscussionResources,
    inbox_identity: &Identity,
) -> ProofResult<()> {
    let watch = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: message_board::ThreadId::from(discussions.primary_root.clone()).into(),
            reader: ReadActor::Identified {
                reader: inbox_identity.clone(),
            },
        })
        .await?;
    if watch.watch_status.is_none_or(|status| !status.watching) {
        return Err("inbox operator did not end with the primary thread watched".into());
    }
    let inbox = proof
        .client
        .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
            protocol_version: ProtocolVersion::CURRENT,
            acting_for: None,
            initializations: Vec::new(),
            scope: InboxScope::Workspace {
                workspace_id: resources.primary_workspace.workspace_id.clone(),
            },
            read_mode: InboxReadMode::Unread,
            reader: inbox_identity.clone(),
            page: PageRequest::default(),
        })?)
        .await?;
    if !inbox.page.records.is_empty() {
        return Err("inbox operator left seeded watched-thread activity unacknowledged".into());
    }
    Ok(())
}

pub(super) async fn grade_participant_process(
    proof: &mut ProofContext,
    discussions: &DiscussionResources,
    creator_identity: &Identity,
    successor_identity: &Identity,
) -> ProofResult<()> {
    let participants = proof
        .client
        .thread_participant_list(ThreadParticipantListRequest {
            thread: message_board::ThreadId::from(discussions.primary_root.clone()).into(),
            page: PageRequest::default(),
        })
        .await?;
    let creator = participants
        .page
        .records
        .iter()
        .find(|participant| participant.identity == *creator_identity)
        .ok_or("Thread creator was not recorded as a Participant")?;
    if creator.role
        != message_board::RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
        || creator.closed_reason != Some(ParticipantClosedReason::Replaced)
        || creator.replaced_by.as_ref() != Some(successor_identity)
    {
        return Err("Orchestrator Leave did not durably hand over to the joined successor".into());
    }
    let successor = participants
        .page
        .records
        .iter()
        .find(|participant| participant.identity == *successor_identity)
        .ok_or("corrected Join did not create the successor Participant")?;
    if successor.role
        != message_board::RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
        || !successor.is_open()
    {
        return Err(
            "handover did not promote the corrected Join participant to open Orchestrator".into(),
        );
    }
    if participants
        .orchestrator
        .as_ref()
        .map(|holder| &holder.identity)
        != Some(successor_identity)
    {
        return Err(
            "participant list did not project the handover successor as Orchestrator".into(),
        );
    }
    let thread = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: message_board::ThreadId::from(discussions.primary_root.clone()).into(),
            reader: ReadActor::Identified {
                reader: successor_identity.clone(),
            },
        })
        .await?;
    if thread
        .thread
        .orchestrator
        .as_ref()
        .map(|holder| &holder.identity)
        != Some(successor_identity)
    {
        return Err("Thread show did not project the handover successor as Orchestrator".into());
    }
    if thread.watch_status.is_none_or(|status| !status.watching) {
        return Err(
            "corrected Join and later explicit Watch did not leave the successor watched".into(),
        );
    }
    let messages = proof
        .client
        .post_list(PostListRequest {
            scope: PostListScope::Thread {
                thread: message_board::ThreadId::from(discussions.primary_root.clone()).into(),
            },
            selection: PostSelection::Latest,
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    if !messages.iter().any(|message| {
        message.actor == *successor_identity && message.text.as_str().contains("checkpoint")
    }) {
        return Err("corrected Join participant did not post the required checkpoint".into());
    }
    Ok(())
}

pub(super) async fn grade_lifecycle(
    proof: &mut ProofContext,
    resources: &NamedResources,
    discussions: &DiscussionResources,
) -> ProofResult<()> {
    let board = proof
        .client
        .board_show(BoardShowRequest {
            board_id: resources.primary_board.board_id.clone(),
        })
        .await?
        .board;
    if board.state != BoardState::Archived {
        return Err("lifecycle operator did not archive the primary board".into());
    }
    let repositories = proof
        .client
        .repository_list(RepositoryListRequest {
            workspace_id: resources.primary_workspace.workspace_id.clone(),
            page: PageRequest::default(),
        })
        .await?
        .page
        .records;
    if !repositories.is_empty() {
        return Err("lifecycle operator did not detach the primary repository association".into());
    }
    let thread = proof
        .client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: message_board::ThreadId::from(discussions.primary_root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?
        .thread;
    if thread.state != ThreadState::Open {
        return Err(
            "lifecycle operator did not restore the thread to unresolved before archive".into(),
        );
    }
    proof
        .client
        .post_show(PostShowRequest {
            post_id: discussions.primary_root.clone(),
        })
        .await?;
    Ok(())
}

trait PlacementThreadRoot {
    fn thread_root(self) -> ProofResult<ThreadId>;
}

impl PlacementThreadRoot for Placement {
    fn thread_root(self) -> ProofResult<ThreadId> {
        match self {
            Placement::Thread { thread_id } => Ok(thread_id),
            Placement::Project { .. } => Err("expected thread placement".into()),
        }
    }
}
