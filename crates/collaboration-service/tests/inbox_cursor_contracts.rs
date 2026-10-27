#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; Result propagates fixture errors."
)]
async fn inbox_cursors_bind_scope_mode_reader_and_store_without_initialization_on_error()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("inbox cursor proof").await?;
    let (workspace, board, project) = create_location(&fixture, "cursor binding").await?;
    let mut original_posts = Vec::new();
    for author in ["first", "second"] {
        original_posts.push(
            post_at_placement(
                &fixture,
                Placement::Project {
                    project_id: project.clone(),
                },
                json!({"actor":{"kind":"human","humanId":author},"text":author}),
            )
            .await?,
        );
    }
    let reader = human_reader("reader")?;
    let mut request = inbox_request(
        InboxScope::Project {
            project_id: project.clone(),
        },
        reader.clone(),
        InboxReadMode::Latest,
    )?;
    request.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(request.clone()).await?;
    let mut continuation = request.clone();
    continuation.page.cursor = Some(first.page.next_cursor.ok_or("cursor")?);
    let mut scope = continuation.clone();
    scope.scope = InboxScope::Workspace {
        workspace_id: workspace.clone(),
    };
    let mut mode = continuation.clone();
    mode.read_mode = InboxReadMode::Unread;
    let mut other = continuation.clone();
    other.reader = human_reader("other-reader")?;
    let mut malformed = request;
    malformed.read_mode = InboxReadMode::Unread;
    malformed.page.cursor = Some("not-a-cursor".to_owned());
    for invalid in [scope, mode, other, malformed] {
        // Each invalid query also carries a real independently keyed initialization.
        // Cursor rejection must happen before its project write.
        let mut props: InboxFetchRequestProps = invalid.into();
        if props.read_mode == InboxReadMode::Unread {
            props.initializations = vec![ReadInitializationEntry {
                project_id: project.clone(),
                request_id: RequestId::generate(),
            }];
        }
        assert!(
            matches!(client.inbox_fetch(InboxFetchRequest::new(props)?).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidCursor)
        );
    }
    for name in ["reader", "other-reader"] {
        assert!(
            client
                .inbox_workspaces(InboxWorkspacesRequest {
                    reader: human_reader(name)?,
                    unread_only: false,
                    page: PageRequest::default()
                })
                .await?
                .page
                .records
                .is_empty()
        );
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut snapshot = store.snapshot().await?;
    assert!(snapshot.records(RecordKind::ReadMark).await?.is_empty());
    snapshot.finish().await?;
    let foreign = BoardFixture::start().await?;
    for (tool, fields) in [
        (
            "workspace_create",
            json!({"workspaceId":workspace,"name":"cursor binding","description":"workspace description"}),
        ),
        (
            "board_create",
            json!({"workspaceId":workspace,"boardId":board,"name":"cursor binding","description":"board description","kind":{"kind":"area"}}),
        ),
        (
            "project_create",
            json!({"boardId":board,"projectId":project,"title":"cursor binding"}),
        ),
    ] {
        let mut create = foreign.header();
        create
            .as_object_mut()
            .ok_or("request")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        if tool == "project_create" {
            foreign
                .prepare_registered_creation(&serde_json::from_value(create.clone())?)
                .await?;
        }
        accepted(&foreign.api.call(tool, create).await?);
    }
    for (author, id) in ["first", "second"].into_iter().zip(original_posts) {
        let mut post = foreign.header();
        post["actor"] = json!({"kind":"human","humanId":author});
        post["postId"] = json!(id);
        post["projectId"] = json!(project);
        post["about"] = json!(foreign.discussion_subject(&project).await?);
        post["role"] = json!(null);
        post["watch"] = json!(false);
        post["text"] = json!(author);
        post["references"] = json!([]);
        post["addressees"] = json!([]);
        accepted(&foreign.api.call("thread_create", post).await?);
    }
    let foreign_client = foreign.api.client("foreign cursor proof").await?;
    assert!(
        matches!(foreign_client.inbox_fetch(continuation.clone()).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidCursor)
    );
    assert_eq!(
        client.inbox_fetch(continuation).await?.page.records.len(),
        1
    );
    foreign.stop().await?;
    assert!(
        client
            .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
            .await?
            .page
            .records
            .is_empty()
    );
    let mut expected = Vec::new();
    for author in ["third", "fourth"] {
        expected.push(
            post_at_placement(
                &fixture,
                Placement::Project {
                    project_id: project.clone(),
                },
                json!({"actor":{"kind":"human","humanId":author},"text":author}),
            )
            .await?,
        );
    }
    let mut unread = inbox_request(
        InboxScope::Project {
            project_id: project.clone(),
        },
        reader,
        InboxReadMode::Unread,
    )?;
    unread.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(unread.clone()).await?;
    assert_eq!(
        inbox_post_ids(&first.page.records),
        vec![expected.first().ok_or("third")?.clone()]
    );
    unread.page.cursor = Some(first.page.next_cursor.ok_or("unread continuation")?);
    let later = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project,
        },
        json!({"actor":{"kind":"human","humanId":"after-upper"},"text":"after upper"}),
    )
    .await?;
    let second = client.inbox_fetch(unread.clone()).await?;
    assert_eq!(
        inbox_post_ids(&second.page.records),
        vec![expected.get(1).ok_or("fourth")?.clone()]
    );
    assert!(second.page.next_cursor.is_none());
    unread.page = PageRequest::default();
    let fresh = client.inbox_fetch(unread).await?;
    assert_eq!(fresh.page.records.len(), 3);
    assert!(inbox_post_ids(&fresh.page.records).contains(&later));
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove behavior; Result propagates fixture errors."
)]
async fn first_unread_initialization_and_post_assign_activity_to_history_or_unread_atomically()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("first-use race proof").await?;
    let (_, _, project) = create_location(&fixture, "first use").await?;
    let reader = human_reader("reader")?;
    let barrier = tokio::sync::Barrier::new(2);
    let reading = async {
        barrier.wait().await;
        client
            .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
            .await
            .map_err(TestError::from)
    };
    let writing = async {
        barrier.wait().await;
        post_at_placement(
            &fixture,
            Placement::Project {
                project_id: project.clone(),
            },
            json!({"actor":{"kind":"human","humanId":"writer"},"text":"racing first use"}),
        )
        .await
    };
    let (initial, post) = tokio::join!(reading, writing);
    let initial = initial?;
    let post = post?;
    let ProjectWriteCallResult::Replied { reply } = &initial
        .page
        .initializations
        .first()
        .ok_or("initialization")?
        .outcome
    else {
        return Err("initialization unanswered".into());
    };
    let boundary = reply.reader.as_ref().ok_or("read boundary")?.read_through;
    let post = client.post_show(PostShowRequest { post_id: post }).await?.0;
    let RecordPlacement::Project(position) = post.position else {
        return Err("position".into());
    };
    let later = client
        .inbox_fetch(inbox_request(
            InboxScope::Project {
                project_id: project,
            },
            reader,
            InboxReadMode::Unread,
        )?)
        .await?;
    let included = inbox_post_ids(&later.page.records).contains(&post.post_id);
    assert_eq!(included, position.cursor.position > boundary.position);
    assert_eq!(later.page.records.len(), usize::from(included));
    fixture.stop().await
}
