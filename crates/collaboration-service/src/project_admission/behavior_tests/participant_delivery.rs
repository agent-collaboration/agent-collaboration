use super::membership_completion::join_operation;
use super::participant_intervals::roster;
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Delivery keeps declared presence monotonic; bookmarks now bind complete feed coverage."
)]
async fn subscription_handoff_keeps_last_seen_monotonic_and_participant_changes_stay_out_of_inbox()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
    fixture
        .admit(
            reader.clone(),
            join_operation(&fixture.root, ParticipantRole::Reviewer),
            fixture.now,
        )
        .await?;
    let joined = roster(&fixture)
        .await?
        .first()
        .ok_or("participant")?
        .joined_at_position;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: SubscriptionScope::thread(fixture.root.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
                    when_idle: Some(WhenIdle::Hold),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(0),
                        cap_seconds: Some(0),
                    },
                    ..Default::default()
                },
            },
            fixture.now,
        )
        .await?;
    let external = fixture
        .post_at(human("external")?, "external", fixture.now)
        .await?;
    let own = fixture
        .post_at(reader.clone(), "own newer presence", fixture.now)
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let at = fixture.now + Duration::seconds(120);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    assert_eq!(roots, vec![fixture.root.clone()]);
    let (notice, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 1);
    let row = notice.roots.first().ok_or("root notice")?;
    assert_eq!(Some(row.through), external.position.project_cursor());
    assert!(row.from.position > joined.position);
    assert!(!serde_json::to_string(&notice)?.contains("external"));
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let participant = roster(&fixture)
        .await?
        .into_iter()
        .find(|participant| participant.identity == reader)
        .ok_or("participant")?;
    assert_eq!(
        Some(participant.last_seen_position),
        own.position.project_cursor()
    );
    // E17/VW-6 changed the bookmark to coverage of a project's complete feed.
    // The keyed mark may cover the join position; the join itself is still not inbox activity.
    fixture
        .admit(
            human("owner")?,
            BoardOperation::ReadMarkAdvance {
                reader: human("owner")?,
                scope: ReadScope::Thread {
                    thread_id: fixture.root.clone(),
                },
                through: joined,
            },
            fixture.now,
        )
        .await?;
    let observer = human("observer")?;
    fixture
        .admit(
            observer.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    let mut join = join_operation(&fixture.root, ParticipantRole::Advisor);
    if let BoardOperation::ThreadJoin { watch, .. } = &mut join {
        *watch = false;
    }
    fixture
        .admit(session("later-participant")?, join, fixture.now)
        .await?;
    let latest = fixture
        .application
        .board()
        .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
            protocol_version: ProtocolVersion::CURRENT,
            acting_for: None,
            reader: observer,
            scope: InboxScope::Project {
                project_id: fixture.project.clone(),
            },
            read_mode: InboxReadMode::Latest,
            initializations: Vec::new(),
            page: PageRequest::default(),
        })?)
        .await?;
    assert!(
        latest
            .page
            .records
            .iter()
            .all(|record| matches!(record, InboxActivity::PostCreated { .. }))
    );
    assert_eq!(latest.page.records.len(), 3);
    fixture.stop().await
}
