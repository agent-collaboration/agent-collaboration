use super::*;

async fn assert_unread_agreement(
    fixture: &SubscriptionFixture,
    reader: &Identity,
    expected: bool,
) -> Result<(), TestError> {
    let has_unread = !fixture.unread_posts(reader.clone()).await?.is_empty();
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
    assert_eq!(has_unread, expected);
    assert_eq!(
        summaries.page.records.first().ok_or("summary")?.has_unread,
        has_unread
    );
    Ok(())
}

#[tokio::test]

async fn rewatch_boundaries_and_acknowledgements_keep_summary_equal_to_inbox()
-> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let reader = fixture.reader.clone();
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
    let watch = BoardOperation::ThreadWatch {
        thread_id: fixture.root.clone(),
    };
    fixture
        .admit(reader.clone(), watch.clone(), fixture.now)
        .await?;
    fixture
        .post_at(human("author")?, "first", fixture.now)
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
    assert_unread_agreement(&fixture, &reader, false).await?;
    fixture
        .admit(reader.clone(), watch.clone(), fixture.now)
        .await?;
    assert_unread_agreement(&fixture, &reader, false).await?;
    let second = fixture
        .post_at(human("author")?, "second", fixture.now)
        .await?;
    fixture.admit(reader.clone(), watch, fixture.now).await?;
    assert_unread_agreement(&fixture, &reader, true).await?;
    let RecordPlacement::Project(through) = second.position else {
        return Err("post position".into());
    };
    fixture
        .admit(
            reader.clone(),
            BoardOperation::ReadMarkAdvance {
                reader: reader.clone(),
                scope: ReadScope::Thread {
                    thread_id: fixture.root.clone(),
                },
                through: through.cursor,
            },
            fixture.now,
        )
        .await?;
    assert_unread_agreement(&fixture, &reader, false).await?;
    fixture.stop().await
}
