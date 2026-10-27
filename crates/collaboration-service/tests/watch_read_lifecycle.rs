#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
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
    reason = "Watch, scoped marks, state changes and unread summaries survive actual store/API reopening."
)]
async fn future_only_watches_scoped_acknowledgements_and_unread_summaries_survive_reopen()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("durable reader proof").await?;
    let (workspace, _, project) = create_location(&fixture, "durable inbox").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"historical root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let reader = human_reader("reader")?;
    let initialized = client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    assert!(initialized.page.records.is_empty());
    assert_eq!(initialized.page.initializations.len(), 1);
    accepted(
        &keyed_call(
            &fixture,
            "thread_watch",
            json!({"actor":reader,"thread":{"thread":thread}}),
        )
        .await?,
    );
    assert!(
        client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread.clone()).into(),
                reader: ReadActor::Identified {
                    reader: reader.clone()
                }
            })
            .await?
            .watch_status
            .ok_or("watch")?
            .earlier_unwatched_range
            .is_none()
    );
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"new reply"}),
    )
    .await?;
    let RecordPlacement::Project(reply_position) = client
        .post_show(PostShowRequest {
            post_id: reply.clone(),
        })
        .await?
        .0
        .position
    else {
        return Err("post position".into());
    };
    accepted(
        &keyed_call(
            &fixture,
            "thread_resolve",
            json!({"thread":{"thread":thread},"reason":"done"}),
        )
        .await?,
    );
    let mut request = inbox_request(
        InboxScope::Project {
            project_id: project.clone(),
        },
        reader.clone(),
        InboxReadMode::Unread,
    )?;
    request.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(request.clone()).await?;
    assert_eq!(inbox_post_ids(&first.page.records), vec![reply]);
    request.page.cursor = Some(first.page.next_cursor.ok_or("state change continuation")?);
    let acknowledge = |through: FeedCursor| json!({"protocolVersion":1,"actor":reader,"actingFor":null,"entries":[{"requestId":RequestId::generate(),"scope":{"kind":"thread","thread":{"thread":thread}},"through":through}]});
    let ack = fixture
        .api
        .call("inbox_acknowledge", acknowledge(reply_position.cursor))
        .await?;
    assert_eq!(
        ack.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    let continued = client.inbox_fetch(request).await?;
    assert_eq!(continued.page.records.len(), 1);
    assert!(
        matches!(continued.page.records.first(), Some(InboxActivity::ThreadStateChanged { state: ThreadState::Resolved, thread_id, .. }) if thread_id == &thread)
    );
    let summaries = client
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: reader.clone(),
            unread_only: true,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(summaries.page.records.len(), 1);
    assert_eq!(
        summaries
            .page
            .records
            .first()
            .ok_or("summary")?
            .workspace_id,
        workspace
    );
    assert!(summaries.page.records.first().ok_or("summary")?.has_unread);
    let through = *continued.page.high_water.get(&project).ok_or("head")?;
    let ack = fixture
        .api
        .call("inbox_acknowledge", acknowledge(through))
        .await?;
    assert_eq!(
        ack.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    assert!(
        client
            .inbox_workspaces(InboxWorkspacesRequest {
                reader: reader.clone(),
                unread_only: true,
                page: PageRequest::default()
            })
            .await?
            .page
            .records
            .is_empty()
    );
    accepted(
        &keyed_call(
            &fixture,
            "thread_unresolve",
            json!({"thread":{"thread":thread}}),
        )
        .await?,
    );
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("reopened reader proof").await?;
    let summaries = client
        .inbox_workspaces(InboxWorkspacesRequest {
            reader: reader.clone(),
            unread_only: true,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(summaries.page.records.len(), 1);
    assert!(summaries.page.records.first().ok_or("summary")?.has_unread);
    let unread = client
        .inbox_fetch(inbox_request(
            InboxScope::Project {
                project_id: project,
            },
            reader.clone(),
            InboxReadMode::Unread,
        )?)
        .await?;
    assert_eq!(unread.page.records.len(), 1);
    assert!(matches!(
        unread.page.records.first(),
        Some(InboxActivity::ThreadStateChanged {
            state: ThreadState::Open,
            ..
        })
    ));
    assert!(
        client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread).into(),
                reader: ReadActor::Identified { reader }
            })
            .await?
            .watch_status
            .ok_or("watch")?
            .watching
    );
    fixture.stop().await
}
