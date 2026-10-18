//! A broken native project cannot take down a healthy sibling's public operations.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;

#[allow(clippy::indexing_slicing)] // Constructing fields in a test-owned JSON object.
async fn corrupt_sibling() -> Result<(BoardFixture, ProjectId, ProjectId, ThreadId), TestError> {
    let fixture = BoardFixture::start().await?;
    let (healthy, _) = fixture.project().await?;
    let (broken, _) = fixture.project().await?;
    let root = PostId::generate();
    let mut request = fixture.header();
    request["projectId"] = json!(healthy);
    request["postId"] = json!(root);
    request["about"] = json!({"kind":"project","subject":{"line":"Healthy conversation"}});
    request["text"] = json!("healthy conversation");
    request["references"] = json!([]);
    request["role"] = json!(null);
    request["watch"] = json!(false);
    request["addressees"] = json!([]);
    request["observedEpoch"] = json!(1);
    accepted(&fixture.api.call("thread_create", request).await?);
    let fixture = fixture.restart_read_fixture().await?;
    std::fs::write(
        fixture
            ._directory
            .path()
            .join("projects")
            .join(format!("{}.sqlite", broken.as_str())),
        b"deliberately corrupt isolated project",
    )?;
    Ok((
        fixture,
        healthy,
        broken,
        ThreadId::try_from(root.as_str().to_owned())?,
    ))
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn creation_recovery_contains_a_corrupt_sibling() -> Result<(), TestError> {
    let (fixture, healthy, broken, _) = corrupt_sibling().await?;
    assert!(fixture._admission.recover_creating_projects().await.is_ok());
    accepted(
        &fixture
            .api
            .call(
                "workstream_create",
                fixture.request(
                    &healthy,
                    json!({
                        "kind":"workstreamCreate","workstreamId":WorkstreamId::generate(),
                        "name":"still writable","goal":"contained failure"
                    }),
                ),
            )
            .await?,
    );
    let bad = fixture
        .api
        .call(
            "hook_card",
            json!({
                "projectId":broken,"actor":fixture.actor,"subject":{"kind":"project"}
            }),
        )
        .await?;
    assert_eq!(bad.pointer("/result/kind"), Some(&json!("unavailable")));
    assert_eq!(
        bad.pointer("/result/payload/reason/project"),
        Some(&json!(broken))
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn healthy_thread_routing_survives_a_corrupt_sibling() -> Result<(), TestError> {
    let (fixture, healthy, broken, thread) = corrupt_sibling().await?;
    let mut request = fixture.header();
    request["postId"] = json!(PostId::generate());
    request["placement"] = json!({"kind":"thread","thread":{"thread":thread,"project":healthy}});
    request["text"] = json!("healthy sibling write");
    request["references"] = json!([]);
    request["addressees"] = json!([]);
    request["observedEpoch"] = json!(1);
    accepted(&fixture.api.call("thread_post", request).await?);
    let missing = fixture
        .api
        .call(
            "thread_show",
            json!({
                "thread":{"thread":ThreadId::generate(),"project":broken},"reader":{"kind":"anonymous"}
            }),
        )
        .await?;
    assert_eq!(
        missing.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{missing}"
    );
    let shown = fixture
        .api
        .call(
            "view_work",
            json!({
                "projectId":healthy
            }),
        )
        .await?;
    assert!(shown.get("result").is_some(), "{shown}");
    fixture.stop().await
}
