//! Narrow views are not a corruption hunt through unrelated current records.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn an_unassigned_task_card_does_not_read_an_unrelated_task_body() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let healthy = fixture.task(&project, &workstream).await?;
    let corrupt = fixture.task(&project, &workstream).await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.title', '') WHERE kind = 'task' AND id = ?").bind(corrupt.as_str()).execute(&mut observer).await?;
    let card = fixture.api.call("view_card", json!({"projectId":project,"actor":{"kind":"human","humanId":"unassigned"},"subject":{"kind":"task","subject":healthy}})).await?;
    assert_eq!(
        card.pointer("/result/subject"),
        Some(&json!(RecordRef::from(healthy))),
        "{card}"
    );
    assert_eq!(card.pointer("/result/counts/task"), Some(&json!(2)));
    let mut read = store.snapshot().await?;
    assert!(read.record(&corrupt.into()).await.is_err());
    read.finish().await?;
    observer.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn owner_and_lead_load_do_not_load_an_irrelevant_projects_bodies() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let (_, _, foreign) = create_location(&fixture, "another lead, no question").await?;
    let other = json!({"kind":"human","humanId":"other lead"});
    let mut grant = fixture.request(&foreign, json!({"kind":"roleGrant","subject":{"kind":"project","subject":foreign},"role":"lead","holder":other,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}));
    grant
        .as_object_mut()
        .ok_or("grant")?
        .insert("actor".to_owned(), other.clone());
    accepted(&fixture.api.call("role_grant", grant).await?);
    let corrupt = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: foreign.clone(),
        },
        json!({"text":"unused post"}),
    )
    .await?;
    let store = fixture
        ._registry
        .existing_project(&foreign)
        .await?
        .ok_or("foreign")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.text', '') WHERE kind = 'post' AND id = ?").bind(corrupt.as_str()).execute(&mut observer).await?;
    let lead = json!({"actor":{"kind":"declared","identity":fixture.actor},"installation":"00000000-0000-4000-8000-000000000001","machine":"proof"});
    for (tool, request) in [
        ("view_owner", json!({})),
        ("view_lead_load", json!({"lead":lead})),
        (
            "view_card",
            json!({"projectId":project,"actor":fixture.actor,"subject":{"kind":"project"}}),
        ),
    ] {
        let answer = fixture.api.call(tool, request).await?;
        assert!(answer.get("result").is_some(), "{tool}: {answer}");
        if tool != "view_card" {
            assert_eq!(
                answer.pointer("/result/unavailableProjects"),
                Some(&json!([])),
                "{tool}: {answer}"
            );
        }
    }
    let mut read = store.snapshot().await?;
    assert!(read.record(&corrupt.into()).await.is_err());
    read.finish().await?;
    observer.close().await?;
    fixture.stop().await
}
