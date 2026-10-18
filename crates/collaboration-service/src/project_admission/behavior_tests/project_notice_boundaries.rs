use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn empty_project_subscription_covers_its_first_later_root_with_a_pending_window()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::empty().await?;
    let reader = fixture.reader.clone();
    let mut read = fixture.project_store().await?.snapshot().await?;
    let boundary = read.high_water();
    assert!(read.records(RecordKind::Post).await?.is_empty());
    read.finish().await?;
    let scope = SubscriptionScope::project(fixture.project.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: scope.clone(),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Poll),
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
    assert_eq!(
        fixture
            .subscription(&reader, &scope, fixture.now)
            .await?
            .ok_or("subscription")?
            .state(),
        SubscriptionState::Active
    );
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert_eq!(read.project_watch(&reader).await?, Some(boundary));
    read.finish().await?;
    fixture
        .create_root(
            &fixture.root,
            human("first-root-author")?,
            "first root after subscription",
        )
        .await?;
    fixture.rescan(&reader, fixture.now).await?;
    let show = fixture
        .application
        .board()
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (fixture.root.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?;
    let watch = show.watch_status.ok_or("watch")?;
    assert!(watch.watching);
    let mut read = fixture.project_store().await?.snapshot().await?;
    let post = read
        .record(&PostId::try_from(fixture.root.as_str().to_owned())?.into())
        .await?
        .ok_or("post")?;
    read.finish().await?;
    let RecordPosition::Project(post_cursor) = post.envelope().position else {
        return Err("position".into());
    };
    assert_eq!(watch.starts_after, Some(boundary));
    assert!(boundary.position < post_cursor.position);
    let subscription = fixture
        .subscription(&reader, &scope, fixture.now)
        .await?
        .ok_or("subscription")?;
    assert_eq!(
        subscription
            .roots()
            .iter()
            .find(|root| root.thread_id() == &fixture.root)
            .map(SubscriptionRootRecord::pending_count),
        Some(1)
    );
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, fixture.now)
        .await?;
    assert_eq!(roots, vec![fixture.root.clone()]);
    let (notice, _) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, fixture.now, usize::MAX)
        .await?;
    assert_eq!(
        notice.roots.first().map(|root| &root.thread_id),
        Some(&fixture.root)
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn p2_project_root_notice_settlement_keeps_inbox_fetch_and_ack_valid() -> Result<(), TestError>
{
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
    let scope = SubscriptionScope::project(fixture.project.clone());
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ReadMarkInitialize {
                reader: reader.clone(),
                scope: ReadScope::Project {
                    project_id: fixture.project.clone(),
                },
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope,
                policy: SubscriptionPolicyPatch::default(),
            },
            fixture.now,
        )
        .await?;
    let root = ThreadId::generate();
    fixture
        .create_root_at(
            &root,
            human("review-p2-author")?,
            "new root",
            fixture.now + Duration::seconds(1),
        )
        .await?;
    fixture
        .rescan(&reader, fixture.now + Duration::seconds(1))
        .await?;
    let at = fixture.now + Duration::seconds(200);
    let roots = fixture.windows.due_subscription_roots(&reader, at).await?;
    assert!(roots.contains(&root));
    let (batch, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, at, usize::MAX)
        .await?;
    assert!(batch.roots.iter().any(|notice| notice.thread_id == root));
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    let inbox = fixture
        .application
        .board()
        .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
            protocol_version: ProtocolVersion::CURRENT,
            acting_for: None,
            reader: reader.clone(),
            scope: InboxScope::Project {
                project_id: fixture.project.clone(),
            },
            read_mode: InboxReadMode::Unread,
            initializations: Vec::new(),
            page: PageRequest::default(),
        })?)
        .await?;
    let (scope, through) = inbox
        .page
        .records
        .iter()
        .find_map(|record| match record {
            InboxActivity::PostCreated {
                post,
                acknowledgement_scope,
                ..
            } if post.post_id.as_str() == root.as_str() => {
                Some((acknowledgement_scope.clone(), post.position))
            }
            _ => None,
        })
        .ok_or("new root unread")?;
    let RecordPosition::Project(through) = through else {
        return Err("position".into());
    };
    let ack = fixture
        .application
        .board()
        .inbox_acknowledge(InboxAcknowledgeRequest::new(
            InboxAcknowledgeRequestProps {
                protocol_version: ProtocolVersion::CURRENT,
                actor: reader,
                acting_for: None,
                entries: vec![AcknowledgementEntry {
                    request_id: RequestId::generate(),
                    resubmits: None,
                    observed_epoch: Some(EpochNumber::try_from(1)?),
                    scope: scope.into(),
                    through,
                }],
            },
        )?)
        .await?;
    assert_eq!(ack.outcomes.len(), 1);
    let ProjectWriteCallResult::Replied { reply } = &ack.outcomes.first().ok_or("ack")?.outcome
    else {
        return Err("acknowledgement unanswered".into());
    };
    assert_accepted(reply);
    fixture.stop().await
}
