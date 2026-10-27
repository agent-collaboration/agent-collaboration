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
use serde_json::{Value, json};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Repository and directory metadata retain identity, idempotency and archive gates.
async fn metadata_and_repository_operations_preserve_identity_and_lifecycle()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (workspace, board, project) = create_location(&fixture, "Implementation").await?;
    let repository = RepositoryRef::Origin {
        normalized_origin: NormalizedOrigin::try_from("github.com/example/router".to_owned())?,
    };
    for change in ["changed", "alreadySo"] {
        let response = keyed_call(
            &fixture,
            "repository_attach",
            json!({"workspaceId":workspace,"repository":repository}),
        )
        .await?;
        accepted(&response);
        assert_eq!(
            response.pointer("/result/result/decision/change"),
            Some(&json!(change))
        );
    }
    let filtered = fixture
        .api
        .call("workspace_list", json!({"repository":repository,"page":{}}))
        .await?;
    let rows = filtered
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .ok_or("workspaces")?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows.first().ok_or("workspace")?.get("workspaceId"),
        Some(&json!(workspace))
    );
    let list = fixture
        .api
        .call(
            "repository_list",
            json!({"workspaceId":workspace,"page":{}}),
        )
        .await?;
    assert_eq!(
        list.pointer("/result/page/records"),
        Some(&json!([repository]))
    );
    accepted(
        &keyed_call(
            &fixture,
            "project_update",
            json!({"projectId":project,"title":"Delivery"}),
        )
        .await?,
    );
    let renamed = fixture
        .api
        .call("project_show", json!({"projectId":project}))
        .await?;
    assert_eq!(renamed.pointer("/result/project/id"), Some(&json!(project)));
    assert_eq!(
        renamed.pointer("/result/project/title"),
        Some(&json!("Delivery"))
    );
    accepted(&keyed_call(&fixture, "board_archive", json!({"boardId":board})).await?);
    let blocked = keyed_call(
        &fixture,
        "project_update",
        json!({"projectId":project,"title":"Blocked"}),
    )
    .await?;
    assert_eq!(
        blocked.pointer("/result/result/decision/reasons/0/kind"),
        Some(&json!("archived")),
        "{blocked}"
    );
    accepted(
        &keyed_call(
            &fixture,
            "repository_detach",
            json!({"workspaceId":workspace,"repository":repository}),
        )
        .await?,
    );
    let list = fixture
        .api
        .call(
            "repository_list",
            json!({"workspaceId":workspace,"page":{}}),
        )
        .await?;
    assert_eq!(list.pointer("/result/page/records"), Some(&json!([])));
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Exact records, cursor signatures and closed-thread gates cross the real API/native store.
async fn immutable_posts_threads_references_and_signed_pagination_share_one_project_order()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("immutable history proof").await?;
    let (workspace, board, project) = create_location(&fixture, "history").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root.clone());
    assert!(
        !client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread.clone()).into(),
                reader: ReadActor::Identified {
                    reader: human_reader("owner")?
                }
            })
            .await?
            .watch_status
            .ok_or("watch")?
            .watching
    );
    let reply = post_at_placement(&fixture, Placement::Thread { thread_id: thread.clone() }, json!({"text":"reply","references":[{"kind":"post","postId":root},{"kind":"thread","threadId":thread}]})).await?;
    let request = PostListRequest {
        scope: PostListScope::Board {
            board_id: board.clone(),
        },
        selection: PostSelection::Latest,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.post_list(request.clone()).await?;
    let posted = first.page.records.first().ok_or("reply")?;
    assert_eq!(posted.post_id, reply);
    assert_eq!(Vec::from(posted.references.clone()).len(), 2);
    let cursor = first.page.next_cursor.ok_or("cursor")?;
    let mut continuation = request.clone();
    continuation.page.cursor = Some(cursor.clone());
    let second = client.post_list(continuation.clone()).await?;
    assert_eq!(second.page.records.first().ok_or("root")?.post_id, root);
    let (RecordPlacement::Project(first_position), RecordPlacement::Project(second_position)) = (
        posted.position.clone(),
        second.page.records.first().ok_or("root")?.position.clone(),
    ) else {
        return Err("positions not project-local".into());
    };
    assert!(first_position.cursor.position > second_position.cursor.position);
    let mut bytes = cursor.into_bytes();
    let byte = bytes.first_mut().ok_or("empty cursor")?;
    *byte = if *byte == b'a' { b'b' } else { b'a' };
    let mut tampered = continuation.clone();
    tampered.page.cursor = Some(String::from_utf8(bytes)?);
    assert!(
        matches!(client.post_list(tampered).await, Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::InvalidCursor)
    );
    let foreign = BoardFixture::start().await?;
    for (tool, fields) in [
        (
            "workspace_create",
            json!({"workspaceId":workspace,"name":"history","description":""}),
        ),
        (
            "board_create",
            json!({"workspaceId":workspace,"boardId":board,"name":"history","description":"","kind":{"kind":"area"}}),
        ),
        (
            "project_create",
            json!({"projectId":project,"boardId":board,"title":"history"}),
        ),
        (
            "role_grant",
            json!({"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":foreign.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}),
        ),
        (
            "thread_create",
            json!({"postId":root,"projectId":project,"about":{"kind":"project","subject":{"line":"Foreign root"}},"role":null,"watch":false,"text":"foreign root","references":[],"addressees":[]}),
        ),
        (
            "thread_post",
            json!({"postId":reply,"placement":{"kind":"thread","thread":{"thread":thread}},"text":"foreign reply","references":[],"addressees":[]}),
        ),
    ] {
        let reply = if tool == "project_create" {
            create_registered_project(&foreign, fields).await?
        } else {
            keyed_call(&foreign, tool, fields).await?
        };
        accepted(&reply);
    }
    let foreign_client = foreign.api.client("foreign post cursor").await?;
    assert!(
        matches!(foreign_client.post_list(continuation).await, Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind == BoardFailureKind::InvalidCursor)
    );
    foreign.stop().await?;
    accepted(
        &keyed_call(
            &fixture,
            "thread_resolve",
            json!({"thread":{"thread":thread},"reason":"done"}),
        )
        .await?,
    );
    let denied = keyed_call(&fixture, "thread_post", json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"blocked","references":[],"addressees":[]})).await?;
    assert_eq!(
        denied.pointer("/result/result/decision/reasons/0/kind"),
        Some(&json!("closed")),
        "{denied}"
    );
    accepted(
        &keyed_call(
            &fixture,
            "thread_unresolve",
            json!({"thread":{"thread":thread}}),
        )
        .await?,
    );
    post_at_placement(
        &fixture,
        Placement::Thread { thread_id: thread },
        json!({"text":"allowed"}),
    )
    .await?;
    assert_eq!(
        client
            .post_show(PostShowRequest { post_id: root })
            .await?
            .0
            .text
            .as_str(),
        "root"
    );
    fixture.stop().await
}
