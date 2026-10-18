use super::membership_completion::join_operation;
use super::participant_intervals::{create_as, roster};
use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // All seat transitions cross native admission and immutable feed facts.
async fn replace_handover_and_human_resolve_are_atomic_position_transitions()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::empty().await?;
    let first = session("first")?;
    let second = session("second")?;
    let third = session("third")?;
    assert_accepted(
        &create_as(
            &fixture,
            first.clone(),
            Some(ParticipantRole::Orchestrator),
            true,
        )
        .await?,
    );
    fixture
        .admit(
            second.clone(),
            join_operation(&fixture.root, ParticipantRole::Reviewer),
            fixture.now,
        )
        .await?;
    let mut replace = join_operation(&fixture.root, ParticipantRole::Orchestrator);
    if let BoardOperation::ThreadJoin { replace, .. } = &mut replace {
        *replace = Some(first.clone());
    }
    let self_replace = fixture
        .submit(first.clone(), replace.clone(), fixture.now)
        .await?;
    assert!(refusals(&self_replace)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::SelfReplace {..}))));
    let mut stale = replace.clone();
    if let BoardOperation::ThreadJoin { replace, .. } = &mut stale {
        *replace = Some(session("stale-holder")?);
    }
    let stale = fixture.submit(third.clone(), stale, fixture.now).await?;
    assert!(refusals(&stale)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::StaleOrchestrator {..}))));
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(show.thread.orchestrator.ok_or("holder")?.identity, first);
    if let BoardOperation::ThreadJoin { watch, .. } = &mut replace {
        *watch = false;
    }
    fixture.admit(third.clone(), replace, fixture.now).await?;
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(show.thread.orchestrator.ok_or("holder")?.identity, third);
    let participants = roster(&fixture).await?;
    let old = participants
        .iter()
        .find(|participant| participant.identity == first)
        .ok_or("old holder")?;
    assert_eq!(old.closed_reason, Some(ParticipantClosedReason::Replaced));
    assert_eq!(old.closed_at_position, Some(old.last_seen_position));
    let mut read = fixture.project_store().await?.snapshot().await?;
    let workspace = read
        .record(&fixture.project.clone().into())
        .await?
        .ok_or("project")?
        .envelope()
        .scope
        .workspace
        .clone();
    read.finish().await?;
    let listed = fixture
        .application
        .board()
        .thread_list(ThreadListRequest {
            workspace_id: workspace,
            reader: human("inspector")?,
            watched_only: false,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        listed
            .page
            .records
            .first()
            .ok_or("thread")?
            .thread
            .orchestrator
            .as_ref()
            .ok_or("list holder")?
            .identity,
        third
    );
    fixture
        .admit(
            third.clone(),
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: Some(LeaveFollowUp::HandTo { to: second.clone() }),
            },
            fixture.now,
        )
        .await?;
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified { reader: third },
        })
        .await?;
    assert_eq!(
        show.thread.orchestrator.ok_or("new holder")?.identity,
        second
    );
    assert!(!show.watch_status.ok_or("watch")?.watching);
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ThreadResolve {
                thread_id: fixture.root.clone(),
                reason: ThreadResolutionReason::try_from("done".to_owned())?,
            },
            fixture.now,
        )
        .await?;
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(show.thread.state, ThreadState::Resolved);
    assert!(show.thread.orchestrator.is_none());
    let resolved = roster(&fixture).await?;
    assert!(resolved.iter().all(|participant| !participant.is_open()));
    assert!(
        resolved
            .iter()
            .any(|participant| participant.identity == second
                && participant.closed_reason == Some(ParticipantClosedReason::Resolved))
    );
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ThreadUnresolve {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    assert!(
        roster(&fixture)
            .await?
            .iter()
            .all(|participant| !participant.is_open())
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // The reader opt-out exception does not permit leave-and-resolve work.
async fn archived_leave_resolve_refuses_without_changing_participant_or_watch_state()
-> Result<(), TestError> {
    let fixture = SubscriptionFixture::empty().await?;
    let actor = session("archived-orchestrator")?;
    assert_accepted(
        &create_as(
            &fixture,
            actor.clone(),
            Some(ParticipantRole::Orchestrator),
            true,
        )
        .await?,
    );
    let before = roster(&fixture).await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let board = read
        .record(&fixture.project.clone().into())
        .await?
        .ok_or("project")?
        .envelope()
        .scope
        .board
        .clone()
        .ok_or("board")?;
    read.finish().await?;
    assert_accepted(&fixture.application.board().board_archive(serde_json::from_value(serde_json::json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":human("archiver")?,"boardId":board}))?).await?);
    let denied = fixture
        .submit(
            actor.clone(),
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: Some(LeaveFollowUp::Resolve {
                    reason: ThreadResolutionReason::try_from("done".to_owned())?,
                }),
            },
            fixture.now,
        )
        .await?;
    assert_eq!(refusals(&denied)?, &[Refusal::Archived]);
    assert_eq!(roster(&fixture).await?, before);
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: actor.clone(),
            },
        })
        .await?;
    assert_eq!(show.thread.state, ThreadState::Open);
    assert_eq!(show.thread.orchestrator.ok_or("holder")?.identity, actor);
    assert!(show.watch_status.ok_or("watch")?.watching);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // The combined transition publishes one observed lifecycle change to another reader.
async fn leave_resolve_publishes_unread_to_another_active_watcher() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::empty().await?;
    let actor = session("resolving-orchestrator")?;
    assert_accepted(
        &create_as(
            &fixture,
            actor.clone(),
            Some(ParticipantRole::Orchestrator),
            false,
        )
        .await?,
    );
    let watcher = human("clean-watcher")?;
    fixture
        .admit(
            watcher.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    let before = fixture
        .application
        .board()
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: watcher.clone(),
            unread_only: false,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(before.page.records.len(), 1);
    assert!(!before.page.records.first().ok_or("summary")?.has_unread);
    fixture
        .admit(
            actor,
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: Some(LeaveFollowUp::Resolve {
                    reason: ThreadResolutionReason::try_from("done".to_owned())?,
                }),
            },
            fixture.now,
        )
        .await?;
    let after = fixture
        .application
        .board()
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: watcher.clone(),
            unread_only: false,
            page: PageRequest::default(),
        })
        .await?;
    assert!(after.page.records.first().ok_or("summary")?.has_unread);
    let unread = fixture
        .application
        .board()
        .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
            protocol_version: ProtocolVersion::CURRENT,
            acting_for: None,
            reader: watcher,
            scope: InboxScope::Project {
                project_id: fixture.project.clone(),
            },
            read_mode: InboxReadMode::Unread,
            initializations: Vec::new(),
            page: PageRequest::default(),
        })?)
        .await?;
    assert!(unread.page.records.iter().any(|record|matches!(record,InboxActivity::ThreadStateChanged {thread_id,state:ThreadState::Resolved,..} if thread_id==&fixture.root)));
    fixture.stop().await
}
