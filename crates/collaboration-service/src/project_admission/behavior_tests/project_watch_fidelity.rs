//! U79 keeps plain watches distinct from subscription-driven thread activation.
use super::*;

fn subscribe_project(project: &ProjectId) -> BoardOperation {
    BoardOperation::ThreadSubscribe {
        scope: SubscriptionScope::project(project.clone()),
        policy: SubscriptionPolicyPatch {
            mode: Some(SubscriptionMode::Poll),
            ..SubscriptionPolicyPatch::default()
        },
    }
}

fn inbox_request(
    fixture: &SubscriptionFixture,
    reader: &Identity,
    mode: InboxReadMode,
) -> Result<InboxFetchRequest, TestError> {
    Ok(InboxFetchRequest::new(InboxFetchRequestProps {
        protocol_version: ProtocolVersion::CURRENT,
        acting_for: None,
        reader: reader.clone(),
        scope: InboxScope::Project {
            project_id: fixture.project.clone(),
        },
        read_mode: mode,
        initializations: Vec::new(),
        page: PageRequest::default(),
    })?)
}

#[allow(clippy::panic_in_result_fn)]
async fn assert_workspace_unread(
    fixture: &SubscriptionFixture,
    reader: &Identity,
    expected: bool,
) -> Result<(), TestError> {
    let summaries = fixture
        .application
        .board()
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: reader.clone(),
            unread_only: false,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(summaries.page.records.len(), 1);
    assert_eq!(
        summaries
            .page
            .records
            .first()
            .ok_or("workspace summary")?
            .has_unread,
        expected
    );
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn plain_project_watch_never_covers_replies_state_changes_or_thread_reads()
-> Result<(), TestError> {
    // Arrange: the main bookmark predates new activity; no thread watch exists.
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = human("plain project watcher")?;
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
        .admit(reader.clone(), BoardOperation::ProjectWatch {}, fixture.now)
        .await?;
    fixture
        .post_at(human("reply author")?, "unwatched reply", fixture.now)
        .await?;
    for operation in [
        BoardOperation::ThreadResolve {
            thread_id: fixture.root.clone(),
            reason: ThreadResolutionReason::try_from("resolved".to_owned())?,
        },
        BoardOperation::ThreadUnresolve {
            thread_id: fixture.root.clone(),
        },
    ] {
        fixture
            .admit(human("owner")?, operation, fixture.now)
            .await?;
    }

    // Act/Assert: actual inbox filtering and summary agree, rather than treating
    // project interest as a fallback watch for this unwatched thread.
    let unread = fixture
        .application
        .board()
        .inbox_fetch(inbox_request(&fixture, &reader, InboxReadMode::Unread)?)
        .await?;
    assert!(
        unread.page.records.is_empty(),
        "plain watch leaked replies or state changes: {:?}",
        unread.page.records
    );
    assert_workspace_unread(&fixture, &reader, false).await?;
    let latest = fixture
        .application
        .board()
        .inbox_fetch(inbox_request(&fixture, &reader, InboxReadMode::Latest)?)
        .await?;
    assert_eq!(
        latest.page.records.len(),
        1,
        "latest still includes the selected main post only"
    );
    assert!(
        matches!(latest.page.records.first(), Some(InboxActivity::PostCreated { post, .. }) if post.post_id.as_str() == fixture.root.as_str())
    );
    let store = fixture.project_store().await?;
    let mut read = store.snapshot().await?;
    assert!(
        read.thread_watch(&fixture.root, &reader).await?.is_none(),
        "plain watch must not materialize a thread row on post"
    );
    assert!(
        read.effective_thread_watch(&fixture.root, &reader)
            .await?
            .is_none()
    );
    assert_eq!(
        read.effective_thread_watch_batch(std::slice::from_ref(&fixture.root), &reader)
            .await?
            .get(&fixture.root),
        Some(&None)
    );
    assert!(
        read.thread_page_keys("", 100, Some(&reader))
            .await?
            .is_empty()
    );
    read.finish().await?;

    // Sensitivity: main posts still follow the selected project and bookmark.
    let another_root = ThreadId::generate();
    fixture
        .create_root_at(
            &another_root,
            human("another main author")?,
            "selected main",
            fixture.now,
        )
        .await?;
    assert_eq!(
        fixture.unread_posts(reader.clone()).await?,
        vec![PostId::try_from(another_root.as_str().to_owned())?]
    );
    assert_workspace_unread(&fixture, &reader, true).await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn only_active_unexpired_project_subscribers_materialize_new_thread_watches()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let watcher = human("watcher without subscription")?;
    let subscriber = human("active subscriber")?;
    let expired = human("expired subscriber")?;
    let ended = human("ended subscriber")?;
    fixture
        .admit(
            watcher.clone(),
            BoardOperation::ProjectWatch {},
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            subscriber.clone(),
            subscribe_project(&fixture.project),
            fixture.now,
        )
        .await?;
    let lifetime = SubscriptionPolicy::defaults_for(&expired)
        .lifetime()
        .seconds();
    let expired_at = fixture.now
        - Duration::seconds(
            i64::try_from(lifetime)?
                .checked_add(1)
                .ok_or("lifetime overflow")?,
        );
    fixture
        .admit(
            expired.clone(),
            subscribe_project(&fixture.project),
            expired_at,
        )
        .await?;
    fixture
        .admit(
            ended.clone(),
            subscribe_project(&fixture.project),
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            ended.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: SubscriptionScope::project(fixture.project.clone()),
            },
            fixture.now,
        )
        .await?;

    // The activation creates existing rows; expiry/end retain them. A new post
    // may create a missing row only for the still-live subscription.
    let new_thread = ThreadId::generate();
    fixture
        .create_root_at(
            &new_thread,
            human("new root author")?,
            "new root",
            fixture.now,
        )
        .await?;
    let store = fixture.project_store().await?;
    let mut read = store.snapshot().await?;
    for reader in [&subscriber, &expired, &ended] {
        assert!(
            read.thread_watch(&fixture.root, reader).await?.is_some(),
            "activation did not retain its existing-thread watch for {reader:?}"
        );
    }
    assert!(read.thread_watch(&fixture.root, &watcher).await?.is_none());
    assert!(read.thread_watch(&new_thread, &subscriber).await?.is_some());
    for reader in [&watcher, &expired, &ended] {
        assert!(
            read.thread_watch(&new_thread, reader).await?.is_none(),
            "non-live project subscription materialized a row for {reader:?}"
        );
    }
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn project_subscription_reactivates_disabled_rows_except_a_retained_thread_override()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
    let overridden = ThreadId::generate();
    let already_active = ThreadId::generate();
    fixture
        .create_root_at(
            &overridden,
            human("override root author")?,
            "override",
            fixture.now,
        )
        .await?;
    fixture
        .create_root_at(
            &already_active,
            human("active root author")?,
            "active",
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnwatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadJoin {
                thread_id: overridden.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: Some(SubscriptionMode::Poll),
                when_idle: None,
                replace: None,
                note: None,
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadUnwatch {
                thread_id: overridden.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: already_active.clone(),
            },
            fixture.now,
        )
        .await?;
    let store = fixture.project_store().await?;
    let mut read = store.snapshot().await?;
    let prior_boundary = read
        .thread_watch(&already_active, &reader)
        .await?
        .ok_or("active boundary")?;
    let activation_boundary = read.high_water();
    read.finish().await?;
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: fixture.project.clone(),
        actor: reader.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: subscribe_project(&fixture.project),
    };

    let accepted = fixture.admission.submit_at(&request, fixture.now).await?;
    assert_accepted(&accepted);
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.thread_watch(&fixture.root, &reader).await?,
        Some(activation_boundary),
        "a disabled row with no thread subscription must reactivate"
    );
    assert_eq!(
        read.thread_watch(&already_active, &reader).await?,
        Some(prior_boundary),
        "an active row keeps its boundary"
    );
    assert!(
        read.thread_watch(&overridden, &reader).await?.is_none(),
        "the ended thread-subscription override must remain disabled"
    );
    let retained = read
        .membership(&reader, &SubscriptionScope::thread(overridden.clone()))
        .await?
        .ok_or("retained override")?;
    assert_eq!(
        retained.ended().ok_or("override end")?.reason,
        EndReason::Cancelled
    );
    let after = read.high_water();
    read.finish().await?;
    assert_eq!(
        fixture
            .admission
            .submit_at(&request, fixture.now + Duration::seconds(1))
            .await?
            .result,
        accepted.result
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), after, "keyed replay writes nothing");
    assert_eq!(
        read.thread_watch(&fixture.root, &reader).await?,
        Some(activation_boundary)
    );
    assert!(read.thread_watch(&overridden, &reader).await?.is_none());
    read.finish().await?;
    fixture.stop().await
}
