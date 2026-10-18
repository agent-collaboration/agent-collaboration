#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn project_filter_keeps_global_watches_without_subscribing_to_project_threads()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("filtered inbox proof").await?;
    let (_, board, selected) = create_location(&fixture, "selected").await?;
    let (_, outside_board, outside) = create_location(&fixture, "outside").await?;
    let reader = human_reader("reader")?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: selected.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"root-author"},"text":"old root"}),
    )
    .await?;
    let watched_root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: outside.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"outside-author"},"text":"watched root"}),
    )
    .await?;
    let watched_thread = ThreadId::from(watched_root.clone());
    let mut watch = fixture.header();
    watch["actor"] = json!(reader);
    watch["thread"] = json!({"thread":watched_thread});
    accepted(&fixture.api.call("thread_watch", watch).await?);
    let watched_post = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: watched_thread.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"thread-author"},"text":"watched activity"}),
    )
    .await?;
    let scope = InboxScope::Project {
        project_id: selected.clone(),
    };
    let initial = client
        .inbox_fetch(initialized_request(selected.clone(), reader.clone())?)
        .await?;
    assert_eq!(initial.page.initializations.len(), 1);
    assert_eq!(
        inbox_post_ids(&initial.page.records),
        vec![watched_post.clone()]
    );
    let other_project = ProjectId::generate();
    let mut create = fixture.header();
    create["boardId"] = json!(board);
    create["projectId"] = json!(other_project);
    create["title"] = json!("unrelated");
    accepted(&fixture.api.call("project_create", create).await?);
    let main=post_at_placement(&fixture,Placement::Project {project_id:selected.clone()},json!({"actor":{"kind":"human","humanId":"main-author"},"text":"selected top-level","references":[{"kind":"thread","threadId":watched_thread}]})).await?;
    let unrelated = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: other_project,
        },
        json!({"actor":{"kind":"human","humanId":"unrelated-author"},"text":"not selected"}),
    )
    .await?;
    post_at_placement(&fixture,Placement::Thread {thread_id:ThreadId::from(root.clone())},json!({"actor":{"kind":"human","humanId":"unwatched-author"},"text":"unwatched thread in selected project"})).await?;
    let mut resolve = fixture.header();
    resolve["actor"] = json!({"kind":"human","humanId":"resolver"});
    resolve["thread"] = json!({"thread":watched_thread});
    resolve["reason"] = json!("done");
    accepted(&fixture.api.call("thread_resolve", resolve).await?);
    // The same scope/watch/mark joins must retain rows after catalog reload.
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("reopened filtered inbox proof").await?;
    let mut request = inbox_request(scope.clone(), reader.clone(), InboxReadMode::Unread)?;
    request.page.limit = PageLimit::try_from(1)?;
    let mut records = Vec::new();
    loop {
        let response = client.inbox_fetch(request.clone()).await?;
        assert!(response.page.records.len() <= 1);
        assert_eq!(response.page.scope, scope);
        records.extend(response.page.records);
        request.page.cursor = response.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert_eq!(inbox_post_ids(&records), vec![watched_post, main]);
    assert_eq!(records.len(), 3);
    assert!(
        matches!(records.last(),Some(InboxActivity::ThreadStateChanged {project_id,board_id,thread_id,..}) if *project_id==outside && *board_id==outside_board && *thread_id==watched_thread)
    );
    assert!(
        !client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (ThreadId::from(root.clone())).into(),
                reader: ReadActor::Identified {
                    reader: reader.clone()
                }
            })
            .await?
            .watch_status
            .ok_or("watch")?
            .watching
    );
    let latest = client
        .inbox_fetch(inbox_request(
            InboxScope::Board { board_id: board },
            reader,
            InboxReadMode::Latest,
        )?)
        .await?;
    assert_eq!(latest.page.ordering, PostOrdering::NewestFirst);
    assert!(latest.page.initializations.is_empty());
    let ids = inbox_post_ids(&latest.page.records);
    assert!(ids.contains(&unrelated));
    assert!(ids.contains(&root));
    assert!(!ids.contains(&watched_root));
    assert_eq!(latest.page.records.len(), 4);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn latest_is_nonmutating_and_continues_normally_when_watches_change() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("latest watch proof").await?;
    let (workspace, _, selected) = create_location(&fixture, "selected").await?;
    let (_, _, outside) = create_location(&fixture, "outside").await?;
    let reader = human_reader("reader")?;
    let outside_root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: outside,
        },
        json!({"actor":{"kind":"human","humanId":"outside-author"},"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(outside_root);
    let older = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: selected.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"older-author"},"text":"older"}),
    )
    .await?;
    let newly_watched = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"thread-author"},"text":"between pages"}),
    )
    .await?;
    let newer = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: selected.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"newer-author"},"text":"newer"}),
    )
    .await?;
    let passed=post_at_placement(&fixture,Placement::Thread {thread_id:thread.clone()},json!({"actor":{"kind":"human","humanId":"thread-author"},"text":"newly eligible above the cursor"})).await?;
    let scope = InboxScope::Workspace {
        workspace_id: workspace,
    };
    let mut request = inbox_request(scope.clone(), reader.clone(), InboxReadMode::Latest)?;
    request.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&first.page.records), vec![newer]);
    assert!(
        client
            .inbox_workspaces(InboxWorkspacesRequest {
                reader: reader.clone(),
                unread_only: false,
                page: PageRequest::default()
            })
            .await?
            .page
            .records
            .is_empty()
    );
    let later=post_at_placement(&fixture,Placement::Project {project_id:selected.clone()},json!({"actor":{"kind":"human","humanId":"later-author"},"text":"after the captured upper bound"})).await?;
    request.page.cursor = first.page.next_cursor;
    let mut watch = fixture.header();
    watch["actor"] = json!(reader);
    watch["thread"] = json!({"thread":thread});
    accepted(&fixture.api.call("thread_watch", watch).await?);
    let second = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&second.page.records), vec![newly_watched]);
    request.page.cursor = second.page.next_cursor;
    let third = client.inbox_fetch(request).await?;
    assert_eq!(inbox_post_ids(&third.page.records), vec![older]);
    let fresh = client
        .inbox_fetch(inbox_request(scope, reader.clone(), InboxReadMode::Latest)?)
        .await?;
    let ids = inbox_post_ids(&fresh.page.records);
    assert_eq!(ids.first(), Some(&later));
    assert!(ids.contains(&passed));
    // The explicit current initialization remains independent of the preceding latest read.
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("reopened latest watch proof").await?;
    let initialized = client
        .inbox_fetch(initialized_request(selected, reader)?)
        .await?;
    assert_eq!(initialized.page.initializations.len(), 1);
    assert!(initialized.page.records.is_empty());
    fixture.stop().await
}
