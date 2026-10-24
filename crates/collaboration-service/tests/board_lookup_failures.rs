//! Preserve canonical missing-resource guidance and multi-workspace summary coverage.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn missing_resource_uses_the_canonical_inspection_action() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let missing = ProjectId::generate();
    let failure = fixture
        .api
        .call("project_show", json!({"projectId":missing}))
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("resourceNotFound")),
        "{failure}"
    );
    assert_eq!(
        failure.pointer("/error/data/stage"),
        Some(&json!("inspection"))
    );
    assert_eq!(
        failure.pointer("/error/data/nextAction"),
        Some(&json!("inspectResource"))
    );
    assert_eq!(
        failure.pointer("/error/data/details/resource"),
        Some(&json!({"kind":"project","projectId":missing}))
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn missing_root_uses_inspect_resource_and_summary_reads_cover_many_workspaces()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let missing = ThreadId::generate();
    let failure = fixture
        .api
        .call(
            "thread_show",
            json!({"thread":{"thread":missing},"reader":{"kind":"anonymous"}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRootPost")),
        "{failure}"
    );
    assert_eq!(
        failure.pointer("/error/data/nextAction"),
        Some(&json!("inspectResource"))
    );
    assert_eq!(
        failure.pointer("/error/data/details/resource"),
        Some(&json!({"kind":"thread","threadId":missing}))
    );
    let reader = json!({"kind":"human","humanId":"summary-reader"});
    let mut expected = Vec::new();
    for index in 0..8 {
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let project = ProjectId::generate();
        let post = PostId::generate();
        for (tool, fields) in [
            (
                "workspace_create",
                json!({"workspaceId":workspace,"name":format!("Summary workspace {index}"),"description":""}),
            ),
            (
                "board_create",
                json!({"workspaceId":workspace,"boardId":board,"name":"Summary board","description":"","kind":{"kind":"area"}}),
            ),
            (
                "project_create",
                json!({"boardId":board,"projectId":project,"title":"Summary project"}),
            ),
            (
                "role_grant",
                json!({"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}),
            ),
            (
                "thread_create",
                json!({"projectId":project,"postId":post,"about":{"kind":"project","subject":{"line":"Summary root"}},"text":"root","references":[],"role":null,"watch":false,"addressees":[]}),
            ),
        ] {
            let mut request = fixture.header();
            request
                .as_object_mut()
                .ok_or("request")?
                .extend(fields.as_object().ok_or("fields")?.clone());
            if tool == "project_create" {
                fixture
                    .prepare_registered_creation(&serde_json::from_value(request.clone())?)
                    .await?;
            }
            accepted(&fixture.api.call(tool, request).await?);
        }
        let mut request = fixture.header();
        request.as_object_mut().ok_or("request")?.extend(
            json!({"thread":{"thread":post},"actor":reader})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        accepted(&fixture.api.call("thread_watch", request).await?);
        expected.push(workspace);
    }
    let started = std::time::Instant::now();
    let summaries = fixture
        .api
        .call(
            "inbox_workspaces",
            json!({"reader":reader,"unreadOnly":false,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    let records = summaries
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .ok_or("summaries missing")?;
    assert_eq!(records.len(), 8, "{summaries}");
    for workspace in expected {
        assert!(
            records
                .iter()
                .any(|row| row.get("workspaceId") == Some(&json!(workspace)))
        );
    }
    eprintln!(
        "summary read across 8 watched workspaces: {}us",
        started.elapsed().as_micros()
    );
    fixture.stop().await
}
