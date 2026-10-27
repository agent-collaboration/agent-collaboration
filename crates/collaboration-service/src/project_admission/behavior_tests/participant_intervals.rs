use super::membership_completion::join_operation;
use super::*;

pub(super) async fn roster(fixture: &SubscriptionFixture) -> Result<Vec<Participant>, TestError> {
    let mut read = fixture.project_store().await?.snapshot().await?;
    let participants = read.participants(&fixture.root).await?;
    read.finish().await?;
    Ok(participants)
}
pub(super) async fn create_as(
    fixture: &SubscriptionFixture,
    actor: Identity,
    role: Option<ParticipantRole>,
    watch: bool,
) -> Result<WriteReply, TestError> {
    fixture
        .submit(
            actor,
            BoardOperation::ThreadCreate {
                post_id: PostId::try_from(fixture.root.as_str().to_owned())?,
                about: fixture.discussion_subject.clone(),
                text: PostText::try_from("Root".to_owned())?,
                references: Vec::new(),
                participation: role,
                watch,
                addressees: Vec::new(),
            },
            fixture.now,
        )
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Claims use actual native admission and public holder reads."
)]
async fn implementer_seat_is_unique_replaceable_and_visible_on_thread_reads()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::empty().await?;
    assert_accepted(
        &create_as(
            &fixture,
            session("coordinator")?,
            Some(ParticipantRole::Orchestrator),
            true,
        )
        .await?,
    );
    let first = session("implementer-one")?;
    let second = session("implementer-two")?;
    fixture
        .admit(
            first.clone(),
            join_operation(&fixture.root, ParticipantRole::Implementer),
            fixture.now,
        )
        .await?;
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(
        shown.thread.implementer.map(|holder| holder.identity),
        Some(first.clone())
    );
    let denied = fixture
        .submit(
            second.clone(),
            join_operation(&fixture.root, ParticipantRole::Implementer),
            fixture.now,
        )
        .await?;
    assert!(refusals(&denied)?.iter().any(|reason|matches!(reason,Refusal::Thread(reason) if matches!(reason.as_ref(),ThreadRefusal::ImplementerAlreadyExists {holder,..} if holder==&first))));
    let mut replace = join_operation(&fixture.root, ParticipantRole::Implementer);
    if let BoardOperation::ThreadJoin { replace, .. } = &mut replace {
        *replace = Some(first);
    }
    fixture.admit(second.clone(), replace, fixture.now).await?;
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Anonymous {},
        })
        .await?;
    assert_eq!(
        shown.thread.implementer.map(|holder| holder.identity),
        Some(second.clone())
    );
    let listed = fixture
        .application
        .board()
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (fixture.root.clone()).into(),
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(
        listed.implementer.map(|holder| holder.identity),
        Some(second)
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every join remains historical, with one open interval and explicit watch state."
)]
async fn create_and_repeated_joins_retain_history_and_one_open_participant() -> Result<(), TestError>
{
    let mut fixture = SubscriptionFixture::empty().await?;
    let creator = session("creator")?;
    assert_accepted(
        &create_as(
            &fixture,
            creator.clone(),
            Some(ParticipantRole::Advisor),
            false,
        )
        .await?,
    );
    let created = roster(&fixture).await?;
    assert_eq!(created.len(), 1);
    assert_eq!(
        created.first().ok_or("participant")?.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Advisor)
    );
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: creator.clone(),
            },
        })
        .await?;
    assert!(!shown.watch_status.ok_or("watch")?.watching);
    assert!(shown.thread.orchestrator.is_none());
    let mut join = join_operation(&fixture.root, ParticipantRole::Reviewer);
    if let BoardOperation::ThreadJoin { note, .. } = &mut join {
        *note = Some(ParticipantNote::try_from("kept note".to_owned())?);
    }
    fixture.admit(creator.clone(), join, fixture.now).await?;
    let mut repeat = join_operation(&fixture.root, ParticipantRole::Participant);
    if let BoardOperation::ThreadJoin { watch, .. } = &mut repeat {
        *watch = false;
    }
    fixture.admit(creator.clone(), repeat, fixture.now).await?;
    let participants = roster(&fixture).await?;
    assert_eq!(participants.len(), 3);
    assert_eq!(
        participants.iter().map(|row| row.role).collect::<Vec<_>>(),
        vec![
            RecordedParticipantRole::Recorded(ParticipantRole::Advisor),
            RecordedParticipantRole::Recorded(ParticipantRole::Reviewer),
            RecordedParticipantRole::Recorded(ParticipantRole::Participant)
        ]
    );
    assert_eq!(participants.iter().filter(|row| row.is_open()).count(), 1);
    assert!(
        participants
            .iter()
            .take(2)
            .all(|row| row.closed_reason == Some(ParticipantClosedReason::Rejoined))
    );
    let repeated = participants
        .iter()
        .find(|row| row.is_open())
        .ok_or("participant")?;
    assert_eq!(
        repeated.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Participant)
    );
    assert_eq!(repeated.note.as_ref().ok_or("note")?.as_str(), "kept note");
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: creator.clone(),
            },
        })
        .await?;
    assert!(!shown.watch_status.ok_or("watch")?.watching);
    fixture
        .admit(
            creator.clone(),
            BoardOperation::ThreadLeave {
                thread_id: fixture.root.clone(),
                after_leave: None,
            },
            fixture.now,
        )
        .await?;
    let left = roster(&fixture).await?;
    let left = left
        .iter()
        .find(|row| row.closed_reason == Some(ParticipantClosedReason::Left))
        .ok_or("closed interval")?;
    assert_eq!(left.closed_reason, Some(ParticipantClosedReason::Left));
    assert_eq!(left.closed_at_position, Some(left.last_seen_position));
    fixture
        .admit(
            creator,
            join_operation(&fixture.root, ParticipantRole::Advisor),
            fixture.now,
        )
        .await?;
    let rejoined = roster(&fixture).await?;
    assert_eq!(rejoined.len(), 4);
    let active = rejoined
        .iter()
        .find(|participant| participant.is_open())
        .ok_or("new interval")?;
    assert_eq!(
        active.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Advisor)
    );
    assert_eq!(
        active.note.as_ref().ok_or("retained note")?.as_str(),
        "kept note"
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A human root need not join; a session's refused root creation creates no conversation data."
)]
async fn human_create_without_role_has_no_participant_and_session_create_changes_only_rejection_ledger()
-> Result<(), TestError> {
    let fixture = SubscriptionFixture::empty().await?;
    let denied = create_as(&fixture, session("missing-role")?, None, false).await?;
    assert!(
        refusals(&denied)?
            .iter()
            .any(|reason| matches!(reason,Refusal::InvalidShape {field} if field=="participation"))
    );
    assert_accepted(&create_as(&fixture, human("owner")?, None, true).await?);
    assert!(roster(&fixture).await?.is_empty());
    let shown = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: human("owner")?,
            },
        })
        .await?;
    assert!(shown.watch_status.ok_or("watch")?.watching);
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let before = snapshot.high_water();
    snapshot.finish().await?;
    let denied_post = PostId::generate();
    let result=fixture.application.board().thread_create(serde_json::from_value(serde_json::json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":session("unjoined")?,"actingFor":null,"postId":denied_post,"projectId":fixture.project,"about":fixture.discussion_subject,"role":null,"watch":false,"text":"must create","references":[],"addressees":[]}))?).await?;
    assert!(
        super::subscription_fixture::public_refusals(&result)?
            .iter()
            .any(|reason| matches!(reason,Refusal::InvalidShape {field} if field=="participation"))
    );
    let mut snapshot = fixture.project_store().await?.snapshot().await?;
    assert_eq!(
        snapshot.high_water().position.get(),
        before.position.get() + 1,
        "the terminal rejection owns its one request-ledger entry"
    );
    assert!(
        snapshot
            .record(&denied_post.clone().into())
            .await?
            .is_none()
    );
    assert!(
        snapshot
            .record(&ThreadId::from(denied_post).into())
            .await?
            .is_none()
    );
    assert!(snapshot.participants(&fixture.root).await?.is_empty());
    snapshot.finish().await?;
    fixture.stop().await
}
