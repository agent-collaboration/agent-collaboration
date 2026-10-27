#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Retained literal search, scope/kind independence and bounded immutable history proof.
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
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_client::CollaborationClient;
use message_board::*;
use serde_json::json;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_read_setup::{create_location, post_at_placement};

fn search(scope: PostListScope, query: &str) -> Result<PostSearchRequest, TestError> {
    Ok(PostSearchRequest {
        query: SearchQuery::try_from(query.to_owned())?,
        scope,
        kind: SearchPostKind::Both,
        include_archived: false,
        page: PageRequest::default(),
    })
}
fn post_ids(result: &PostSearchResult) -> Vec<PostId> {
    result
        .page
        .records
        .iter()
        .map(|hit| hit.post.post_id.clone())
        .collect()
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn archive_removes_remaining_search_hits_without_invalidating_the_cursor()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("archive search proof").await?;
    let (workspace, board, archived) = create_location(&fixture, "archived").await?;
    let (_, _, active) = create_location(&fixture, "active").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: archived,
        },
        json!({"text":"root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    let older = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"needle older"}),
    )
    .await?;
    let newer = post_at_placement(
        &fixture,
        Placement::Project { project_id: active },
        json!({"text":"needle newer"}),
    )
    .await?;
    let mut request = search(PostListScope::AllWorkspaces, "needle")?;
    request.page.limit = PageLimit::try_from(1)?;
    let first = client.post_search(request.clone()).await?;
    assert_eq!(post_ids(&first), vec![newer]);
    request.page.cursor = Some(first.page.next_cursor.ok_or("missing post cursor")?);
    let mut discovery = DiscoverySearchRequest {
        query: SearchQuery::try_from("archived".to_owned())?,
        scope: DiscoveryScope::AllWorkspaces,
        kind: DiscoveryKind::All,
        include_archived: false,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.discovery_search(discovery.clone()).await?;
    assert!(
        matches!(first.page.records.first(),Some(DiscoveryHit::Workspace {workspace:row}) if row.workspace_id==workspace)
    );
    discovery.page.cursor = Some(first.page.next_cursor.ok_or("missing discovery cursor")?);
    let mut archive = fixture.header();
    archive["boardId"] = json!(board);
    accepted(&fixture.api.call("board_archive", archive).await?);
    let remaining = client.discovery_search(discovery.clone()).await?;
    assert!(remaining.page.records.is_empty());
    assert!(remaining.page.next_cursor.is_none());
    discovery.include_archived = true;
    assert!(
        matches!(client.discovery_search(discovery).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidCursor)
    );
    let remaining = client.post_search(request.clone()).await?;
    assert!(remaining.page.records.is_empty());
    assert!(remaining.page.next_cursor.is_none());
    request.include_archived = true;
    assert!(
        matches!(client.post_search(request).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidCursor)
    );
    let mut explicit = search(
        PostListScope::Thread {
            thread: (thread).into(),
        },
        "needle",
    )?;
    assert!(
        client
            .post_search(explicit.clone())
            .await?
            .page
            .records
            .is_empty()
    );
    explicit.include_archived = true;
    assert_eq!(post_ids(&client.post_search(explicit).await?), vec![older]);
    let mut discovery = DiscoverySearchRequest {
        query: SearchQuery::try_from("archived".to_owned())?,
        scope: DiscoveryScope::AllWorkspaces,
        kind: DiscoveryKind::All,
        include_archived: false,
        page: PageRequest::default(),
    };
    assert_eq!(
        client
            .discovery_search(discovery.clone())
            .await?
            .page
            .records
            .len(),
        1
    );
    discovery.include_archived = true;
    assert_eq!(
        client.discovery_search(discovery).await?.page.records.len(),
        3
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn discovery_pages_bound_full_parent_context_without_losing_projects() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("discovery byte proof").await?;
    let (workspace, board, _) = create_location(&fixture, "needle-large").await?;
    let description = "\"".repeat(MAX_DESCRIPTION_BYTES);
    for (tool, fields) in [
        (
            "workspace_update",
            json!({"workspaceId":workspace,"name":"needle-large","description":description}),
        ),
        (
            "board_update",
            json!({"boardId":board,"name":"needle-large","description":description}),
        ),
    ] {
        let mut request = fixture.header();
        request
            .as_object_mut()
            .ok_or("request")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        accepted(&fixture.api.call(tool, request).await?);
    }
    for index in 0..24 {
        let mut request = fixture.header();
        request.as_object_mut().ok_or("request")?.extend(json!({"boardId":board,"projectId":ProjectId::generate(),"title":format!("needle {index}")}).as_object().ok_or("fields")?.clone());
        fixture
            .prepare_registered_creation(&serde_json::from_value(request.clone())?)
            .await?;
        accepted(&fixture.api.call("project_create", request).await?);
    }
    let mut request = DiscoverySearchRequest {
        query: SearchQuery::try_from("needle".to_owned())?,
        scope: DiscoveryScope::Workspace {
            workspace_id: workspace,
        },
        kind: DiscoveryKind::ProjectDirectoryEntry,
        include_archived: false,
        page: PageRequest::default(),
    };
    let mut ids = std::collections::HashSet::new();
    let mut pages = 0;
    loop {
        let result = client.discovery_search(request.clone()).await?;
        assert!(
            serde_json::to_vec(&result)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        assert!(!result.page.records.is_empty());
        pages += 1;
        for hit in result.page.records {
            let DiscoveryHit::Project {
                workspace,
                board,
                project,
            } = hit
            else {
                panic!("non-project result")
            };
            assert_eq!(workspace.description.as_str(), description);
            assert_eq!(board.description.as_str(), description);
            assert!(ids.insert(project.id));
        }
        request.page.cursor = result.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 25);
    assert!(pages > 1);
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn searches_keep_text_location_kind_and_reader_state_independent() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client: CollaborationClient = fixture.api.client("search proof").await?;
    let (workspace, board, selected) = create_location(&fixture, "Navigation").await?;
    let (_, _, outside) = create_location(&fixture, "Other").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: selected.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"root-author"},"text":"root without keyword"}),
    )
    .await?;
    let thread = ThreadId::from(root.clone());
    let reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"reply-author"},"text":"Navigation 100%_ É"}),
    )
    .await?;
    let main = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: selected.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"main-author"},"text":"NAVIGATION plain é"}),
    )
    .await?;
    post_at_placement(
        &fixture,
        Placement::Project {
            project_id: outside,
        },
        json!({"actor":{"kind":"human","humanId":"outside-author"},"text":"Navigation outside"}),
    )
    .await?;
    post_at_placement(&fixture,Placement::Thread {thread_id:thread.clone()},json!({"actor":{"kind":"human","humanId":"reference-author"},"text":"reference only","references":[{"kind":"post","postId":main}]})).await?;
    let scope = PostListScope::Project {
        project_id: selected.clone(),
    };
    let result = client
        .post_search(search(scope.clone(), " navigation ")?)
        .await?;
    assert_eq!(post_ids(&result), vec![main.clone(), reply.clone()]);
    assert!(
        result
            .page
            .records
            .iter()
            .all(|hit| hit.workspace_id == workspace && hit.post.project_id == selected)
    );
    for (kind, expected) in [
        (SearchPostKind::TopLevel, main.clone()),
        (SearchPostKind::Thread, reply.clone()),
    ] {
        let mut request = search(scope.clone(), "navigation")?;
        request.kind = kind;
        assert_eq!(
            post_ids(&client.post_search(request).await?),
            vec![expected]
        );
    }
    for literal in ["%_", "É"] {
        assert_eq!(
            post_ids(&client.post_search(search(scope.clone(), literal)?).await?),
            vec![reply.clone()]
        );
    }
    assert_eq!(
        post_ids(&client.post_search(search(scope, "é")?).await?),
        vec![main]
    );
    let mut request = search(
        PostListScope::Thread {
            thread: (thread.clone()).into(),
        },
        "navigation",
    )?;
    assert_eq!(
        post_ids(&client.post_search(request.clone()).await?),
        vec![reply]
    );
    request.kind = SearchPostKind::TopLevel;
    assert!(
        matches!(client.post_search(request).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidField)
    );
    let mut request = DiscoverySearchRequest {
        query: SearchQuery::try_from("NAV".to_owned())?,
        scope: DiscoveryScope::AllWorkspaces,
        kind: DiscoveryKind::All,
        include_archived: false,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let mut observed = Vec::new();
    loop {
        let result = client.discovery_search(request.clone()).await?;
        observed.extend(result.page.records);
        request.page.cursor = result.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert_eq!(observed.len(), 3);
    assert!(observed.iter().any(
        |hit| matches!(hit,DiscoveryHit::Workspace {workspace:row} if row.workspace_id==workspace)
    ));
    assert!(observed.iter().any(|hit|matches!(hit,DiscoveryHit::Board {workspace:parent,board:row} if parent.workspace_id==workspace && row.board_id==board)));
    assert!(observed.iter().any(|hit|matches!(hit,DiscoveryHit::Project {workspace:parent,board:parent_board,project} if parent.workspace_id==workspace && parent_board.board_id==board && project.id==selected)));
    let mut request = DiscoverySearchRequest {
        query: SearchQuery::try_from("nav".to_owned())?,
        scope: DiscoveryScope::AllWorkspaces,
        kind: DiscoveryKind::All,
        include_archived: false,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    request.page.cursor = client
        .discovery_search(request.clone())
        .await?
        .page
        .next_cursor;
    request.query = SearchQuery::try_from("other".to_owned())?;
    assert!(
        matches!(client.discovery_search(request).await,Err(collaboration_client::BoardClientError::Rejected(error)) if error.kind==BoardFailureKind::InvalidCursor)
    );
    let reader: Identity = serde_json::from_value(json!({"kind":"human","humanId":"reader"}))?;
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
    assert!(
        !client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread).into(),
                reader: ReadActor::Identified { reader }
            })
            .await?
            .watch_status
            .ok_or("watch missing")?
            .watching
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn large_post_search_pages_respect_bytes_and_report_scan_cost() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("search byte proof").await?;
    let (_, _, project) = create_location(&fixture, "Large search").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"seed"}),
    )
    .await?;
    let body = format!("needle {}", "x".repeat(MAX_MESSAGE_TEXT_BYTES - 7));
    for _ in 0..32 {
        post_at_placement(
            &fixture,
            Placement::Thread {
                thread_id: ThreadId::from(root.clone()),
            },
            json!({"text":body}),
        )
        .await?;
    }
    let scope = PostListScope::Project {
        project_id: project,
    };
    let started = std::time::Instant::now();
    assert!(
        client
            .post_search(search(scope.clone(), "no-matching-substring")?)
            .await?
            .page
            .records
            .is_empty()
    );
    let no_match = started.elapsed();
    let mut request = search(scope, "needle")?;
    let mut ids = std::collections::HashSet::new();
    let mut pages = 0;
    let started = std::time::Instant::now();
    loop {
        let result = client.post_search(request.clone()).await?;
        assert!(
            serde_json::to_vec(&result)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        assert!(!result.page.records.is_empty());
        pages += 1;
        for hit in result.page.records {
            assert!(ids.insert(hit.post.post_id));
        }
        request.page.cursor = result.page.next_cursor;
        if request.page.cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 32);
    assert!(pages > 1);
    eprintln!(
        "2 MiB history: no-match={}us; matching-chain={}us; pages={pages}",
        no_match.as_micros(),
        started.elapsed().as_micros()
    );
    fixture.stop().await
}
