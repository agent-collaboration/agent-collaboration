#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Missing roots and name collisions must retain their exact typed refusals.
async fn thread_root_failures_stay_specific_and_directory_collisions_create_no_native_file()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (_, board, project) = create_location(&fixture, "Implementation").await?;
    let missing = ThreadId::generate();
    let rejected = keyed_call(&fixture, "thread_post", json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":missing}},"text":"reply","references":[],"addressees":[]})).await?;
    assert_eq!(
        rejected.pointer("/error/data/kind"),
        Some(&json!("invalidRootPost")),
        "{rejected}"
    );
    assert_eq!(
        rejected.pointer("/error/data/details/resource"),
        Some(&json!({"kind":"thread","threadId":missing}))
    );
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"root"}),
    )
    .await?;
    let child = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: ThreadId::from(root),
        },
        json!({"text":"child"}),
    )
    .await?;
    let rejected = keyed_call(&fixture, "thread_post", json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":child}},"text":"nested","references":[],"addressees":[]})).await?;
    assert_eq!(
        rejected.pointer("/error/data/kind"),
        Some(&json!("invalidRootPost")),
        "{rejected}"
    );
    let colliding_project = ProjectId::generate();
    let mut input = fixture.header();
    input.as_object_mut().ok_or("request")?.extend(
        json!({"projectId":colliding_project,"boardId":board,"title":"Implementation"})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    let collision = fixture.api.call("project_create", input.clone()).await?;
    let path = fixture
        ._directory
        .path()
        .join("projects")
        .join(colliding_project.as_str());
    assert!(!path.exists());
    let reply: message_board::public_replies::WriteReply =
        serde_json::from_value(collision.get("result").cloned().ok_or("creating reply")?)?;
    assert_eq!(
        reply.result,
        message_board::public_replies::RequestResult::ScopeUnavailable(
            ScopeRefusal::ProjectRejected {
                project: colliding_project,
                reason: ProjectRejection::Directory {
                    refusal: Refusal::NameConflict {
                        name: ResourceName::try_from("Implementation".to_owned())?,
                        existing: project.into(),
                    }
                }
            },
        )
    );
    let replay = fixture.api.call("project_create", input).await?;
    let replay: message_board::public_replies::WriteReply = serde_json::from_value(
        replay
            .get("result")
            .cloned()
            .ok_or("replayed creating reply")?,
    )?;
    assert_eq!(replay.result, reply.result);
    assert!(!path.exists());
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Changed and already-so replies are followed by real posts, rather than relying on guidance prose.
async fn unresolve_changed_and_already_so_allow_replies_on_an_active_board() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let (_, _, project) = create_location(&fixture, "unresolve").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project,
        },
        json!({"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    accepted(
        &keyed_call(
            &fixture,
            "thread_resolve",
            json!({"thread":{"thread":thread},"reason":"done"}),
        )
        .await?,
    );
    for change in ["changed", "alreadySo"] {
        let outcome = keyed_call(
            &fixture,
            "thread_unresolve",
            json!({"thread":{"thread":thread}}),
        )
        .await?;
        accepted(&outcome);
        assert_eq!(
            outcome.pointer("/result/result/decision/change"),
            Some(&json!(change)),
            "{outcome}"
        );
        post_at_placement(
            &fixture,
            Placement::Thread {
                thread_id: thread.clone(),
            },
            json!({"text":change}),
        )
        .await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Fleet metadata updates preserve project history and reader state.
async fn board_update_preserves_history_watch_bookmark_and_thread_list_filter()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("board rename proof").await?;
    let (workspace, board, project) = create_location(&fixture, "board rename").await?;
    let first = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"first"},"text":"first root"}),
    )
    .await?;
    let second = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"second"},"text":"second root"}),
    )
    .await?;
    let thread = ThreadId::from(first);
    let reader = human_reader("reader")?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    accepted(
        &keyed_call(
            &fixture,
            "thread_watch",
            json!({"thread":{"thread":thread},"actor":reader}),
        )
        .await?,
    );
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"reply"}),
    )
    .await?;
    let unread = client
        .inbox_fetch(inbox_request(
            InboxScope::Project {
                project_id: project.clone(),
            },
            reader.clone(),
            InboxReadMode::Unread,
        )?)
        .await?;
    assert_eq!(inbox_post_ids(&unread.page.records), vec![reply.clone()]);
    let through = unread.page.high_water.get(&project).ok_or("project head")?;
    let ack = fixture.api.call("inbox_acknowledge", json!({"protocolVersion":1,"actor":reader,"actingFor":null,"entries":[{"requestId":RequestId::generate(),"scope":{"kind":"thread","thread":{"thread":thread}},"through":through}]})).await?;
    assert_eq!(
        ack.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    accepted(&keyed_call(&fixture, "board_update", json!({"boardId":board,"name":"Renamed Engineering","description":"Updated description"})).await?);
    let shown = fixture
        .api
        .call("board_show", json!({"boardId":board}))
        .await?;
    assert_eq!(shown.pointer("/result/board/boardId"), Some(&json!(board)));
    assert_eq!(
        shown.pointer("/result/board/name"),
        Some(&json!("Renamed Engineering"))
    );
    let post = client.post_show(PostShowRequest { post_id: reply }).await?;
    assert_eq!(
        post.0.placement,
        Placement::Thread {
            thread_id: thread.clone()
        }
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
            .watching
    );
    assert!(
        client
            .inbox_fetch(inbox_request(
                InboxScope::Project {
                    project_id: project
                },
                reader.clone(),
                InboxReadMode::Unread
            )?)
            .await?
            .page
            .records
            .is_empty()
    );
    let mut request = ThreadListRequest {
        workspace_id: workspace,
        reader,
        watched_only: true,
        page: PageRequest::default(),
    };
    let watched = client.thread_list(request.clone()).await?;
    assert_eq!(watched.page.records.len(), 1);
    assert_eq!(
        watched
            .page
            .records
            .first()
            .ok_or("watched")?
            .thread
            .thread_id,
        thread
    );
    request.watched_only = false;
    let all = client.thread_list(request).await?;
    assert_eq!(all.page.records.len(), 2);
    assert!(
        all.page
            .records
            .iter()
            .any(|row| row.thread.thread_id == ThreadId::from(second.clone()))
    );
    fixture.stop().await
}
