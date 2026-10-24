//! Repeated rejection is one derived item with its repeat count and one Lead obligation.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn repeated_rejections_share_the_first_item_and_increment_the_visible_count()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let first = MoveId::generate();
    let second = MoveId::generate();
    for movement in [&first, &second] {
        let reply=fixture.api.call("move_request",fixture.request(&project,json!({"kind":"moveRequest","moveId":movement,"moveName":"finish","expectedRevision":0,"evidence":[],"reason":"no pinned definition","addressees":[]}))).await?;
        assert_eq!(
            reply.pointer("/result/result/decision/kind"),
            Some(&json!("rejected")),
            "{reply}"
        );
    }
    let view = fixture
        .api
        .call("view_work", json!({"projectId":project}))
        .await?;
    let rows = view
        .pointer("/result/rejectedSinceLastMove")
        .and_then(Value::as_array)
        .ok_or("rejections missing")?;
    assert_eq!(rows.len(), 1, "{view}");
    assert_eq!(
        rows.first().and_then(|row| row.get("moveId")),
        Some(&json!(first))
    );
    assert_eq!(
        rows.first().and_then(|row| row.get("repeatCount")),
        Some(&json!(2))
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store missing")?;
    let mut snapshot = store.snapshot().await?;
    let roles = snapshot.records(RecordKind::RoleAssignment).await?;
    let lead = roles
        .into_iter()
        .find_map(|record| match record {
            Record::RoleAssignment(row) if row.role() == AuthorityRole::Lead => Some(row.holder),
            _ => None,
        })
        .ok_or("lead missing")?;
    let intents = snapshot.records(RecordKind::NotifyIntent).await?;
    assert_eq!(intents.iter().filter(|record|matches!(record,Record::NotifyIntent(intent) if intent.cause==first.clone().into() || intent.cause==second.clone().into())).count(),1);
    let as_of = snapshot.high_water();
    snapshot.finish().await?;
    let load = fixture
        .api
        .call("view_lead_load", json!({"lead":lead}))
        .await?;
    assert_eq!(
        load.pointer("/result/owed"),
        Some(
            &json!([{"project":project,"subject":RecordRef::from(first),"obligation":"answerRejection",
                "source":{"kind":"writer"},"asOf":as_of,
                "replicatedThrough":{"epoch":1,"position":0}}])
        )
    );
    let source=json!({"initialStatus":"draft","edges":[{"name":"finish","from":"draft","to":"done","requiredRole":"lead","guards":[]}]}).to_string();
    let mut register = fixture.header();
    register["workspaceId"] = json!(fixture.workspace);
    register["name"] = json!("rejection-cutoff");
    register["version"] = json!("v1");
    register["source"] = json!(source);
    board_fixture::accepted(&fixture.api.call("process_register", register).await?);
    let definition = json!({"name":"rejection-cutoff","version":"v1","sourceHash":format!("{:x}",Sha256::digest(source.as_bytes()))});
    board_fixture::accepted(&fixture.api.call("project_pin_definition",fixture.request(&project,json!({"kind":"projectPinDefinition","definition":definition,"expectedRevision":0}))).await?);
    board_fixture::accepted(&fixture.api.call("move_request",fixture.request(&project,json!({"kind":"moveRequest","moveId":MoveId::generate(),"moveName":"finish","expectedRevision":1,"evidence":[],"reason":"allowed now","addressees":[]}))).await?);
    let settled = fixture
        .api
        .call("view_work", json!({"projectId":project}))
        .await?;
    assert_eq!(
        settled.pointer("/result/rejectedSinceLastMove"),
        Some(&json!([]))
    );
    assert_eq!(
        fixture
            .api
            .call("view_lead_load", json!({"lead":lead}))
            .await?
            .pointer("/result/owed"),
        Some(&json!([]))
    );
    fixture.stop().await
}
