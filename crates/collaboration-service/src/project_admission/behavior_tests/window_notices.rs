use super::*;

async fn project_subscription(
    fixture: &mut SubscriptionFixture,
    reader: &Identity,
) -> Result<(), TestError> {
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ThreadSubscribe {
                scope: SubscriptionScope::project(fixture.project.clone()),
                policy: SubscriptionPolicyPatch::default(),
            },
            fixture.now,
        )
        .await?;
    fixture.rescan(reader, fixture.now).await?;
    Ok(())
}
fn cursor(post: &Post) -> Result<FeedCursor, TestError> {
    match &post.position {
        RecordPlacement::Project(entry) => Ok(entry.cursor),
        _ => Err("not project position".into()),
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn subscription_notice_returns_only_ranges_and_counts_and_keeps_posts_unread()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let root = ThreadId::generate();
    let content = format!("  {}  \nignored second line", "é".repeat(90));
    fixture
        .create_root_at(
            &root,
            human("root-author")?,
            &content,
            fixture.now + Duration::seconds(1),
        )
        .await?;
    let reader = session("notice-reader")?;
    project_subscription(&mut fixture, &reader).await?;
    let first = fixture
        .post_in_thread_at(
            &root,
            human("first-author")?,
            "first private body",
            fixture.now + Duration::seconds(3),
        )
        .await?;
    fixture
        .post_in_thread_at(
            &root,
            human("second-author")?,
            "second private body",
            fixture.now + Duration::seconds(4),
        )
        .await?;
    let last = fixture
        .post_in_thread_at(
            &root,
            human("first-author")?,
            "third private body",
            fixture.now + Duration::seconds(5),
        )
        .await?;
    let due_at = fixture.now + Duration::seconds(125);
    fixture.rescan(&reader, due_at).await?;
    let roots = fixture
        .windows
        .due_subscription_roots(&reader, due_at)
        .await?;
    assert_eq!(roots, vec![root.clone()]);
    let (notice, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due_at, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 1);
    let row = notice.roots.first().ok_or("root")?;
    assert_eq!(row.thread_id, root);
    assert_eq!(row.project_id, fixture.project);
    assert_eq!(row.from, cursor(&first)?);
    assert_eq!(row.through, cursor(&last)?);
    assert_eq!(row.post_count, 3);
    let encoded = serde_json::to_string(&notice)?;
    for private in [
        "first private body",
        "second private body",
        "third private body",
        content.as_str(),
        "first-author",
        "second-author",
        "authors",
        "rootTitleExcerpt",
    ] {
        assert!(!encoded.contains(private));
    }
    let fetched = fixture
        .application
        .board()
        .post_list(PostListRequest {
            scope: PostListScope::Thread {
                thread: (root).into(),
            },
            selection: PostSelection::Range {
                from: ReadPosition::Project { cursor: row.from },
                to: ReadPosition::Project {
                    cursor: row.through,
                },
            },
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(fetched.page.records.len(), 3);
    assert_eq!(
        fetched.page.records.first().ok_or("post")?.post_id,
        first.post_id
    );
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    assert_eq!(fixture.unread_posts(reader).await?.len(), 3);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn subscription_notice_caps_roots_without_settling_omitted_due_windows()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = session("notice-cap-reader")?;
    project_subscription(&mut fixture, &reader).await?;
    for index in 0..21 {
        fixture
            .create_root_at(
                &ThreadId::generate(),
                human(&format!("root-author-{index}"))?,
                &format!("notice root {index}"),
                fixture.now + Duration::seconds(index + 1),
            )
            .await?;
    }
    let due = fixture.now + Duration::seconds(200);
    fixture.rescan(&reader, due).await?;
    let roots = fixture.windows.due_subscription_roots(&reader, due).await?;
    assert_eq!(roots.len(), 21);
    let (notice, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due, usize::MAX)
        .await?;
    assert_eq!(notice.roots.len(), 20);
    assert_eq!(selection.roots.len(), 20);
    let omitted = roots
        .into_iter()
        .filter(|root| !notice.roots.iter().any(|notice| notice.thread_id == *root))
        .collect::<Vec<_>>();
    assert_eq!(omitted.len(), 1);
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        omitted
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn subscription_notice_byte_budget_selects_a_prefix_before_in_flight_commit()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = session("notice-byte-budget-reader")?;
    project_subscription(&mut fixture, &reader).await?;
    for index in 0..2 {
        fixture
            .create_root_at(
                &ThreadId::generate(),
                human(&format!("budget-author-{index}"))?,
                &format!("budget root {index}"),
                fixture.now + Duration::seconds(index + 1),
            )
            .await?;
    }
    let due = fixture.now + Duration::seconds(200);
    fixture.rescan(&reader, due).await?;
    let roots = fixture.windows.due_subscription_roots(&reader, due).await?;
    assert_eq!(roots.len(), 2);
    let (full, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due, usize::MAX)
        .await?;
    let budget = serde_json::to_vec(&full.roots.iter().take(1).collect::<Vec<_>>())?.len();
    assert!(serde_json::to_vec(&full.roots)?.len() > budget);
    fixture
        .windows
        .release_subscription_batch(&reader, &selection)
        .await?;
    let (bounded, selection) = fixture
        .windows
        .select_subscription_notice(&reader, &roots, due, budget)
        .await?;
    assert_eq!(bounded.roots.len(), 1);
    assert_eq!(selection.roots.len(), 1);
    assert_eq!(serde_json::to_vec(&bounded.roots)?.len(), budget);
    assert_eq!(bounded.roots.first(), full.roots.first());
    let omitted = roots
        .into_iter()
        .find(|root| !bounded.roots.iter().any(|notice| notice.thread_id == *root))
        .ok_or("omitted root")?;
    fixture
        .windows
        .settle_subscription_batch(&reader, &selection, SubscriptionDeliveryOutcome::Accepted)
        .await?;
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        vec![omitted.clone()]
    );
    assert!(
        fixture
            .windows
            .select_subscription_notice(&reader, std::slice::from_ref(&omitted), due, 2)
            .await
            .is_err()
    );
    assert_eq!(
        fixture.windows.due_subscription_roots(&reader, due).await?,
        vec![omitted]
    );
    fixture.stop().await
}
