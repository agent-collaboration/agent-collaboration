#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Caller limits are terminal keyed refusals through real MCP and native admission.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn trail_limits_preserve_maximum_values_and_replay_field_refusals() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project store")?;
    let id = WorkTrailRowId::generate();
    let link = json!({"kind":"repoPath","repository":"example.org/repo","path":"\0".repeat(1024),"commit":null});
    let operation = json!({"kind":"workTrailAdd","rowId":id,"subject":{"kind":"project"},"text":"\"".repeat(65536),"tags":vec!["x".repeat(256);32],"links":vec![link;16]});
    accepted(
        &fixture
            .api
            .call(
                "work_trail_add",
                fixture.request(&project, operation.clone()),
            )
            .await?,
    );
    let mut read = store.snapshot().await?;
    let Some(Record::WorkTrailRow(row)) = read.record(&id.into()).await? else {
        return Err("maximum trail missing".into());
    };
    assert_eq!(row.tags.len(), 32);
    assert_eq!(row.links.len(), 16);
    assert_eq!(row.text.as_str(), "\"".repeat(65536));
    read.finish().await?;
    for field in ["tags", "links", "path", "url"] {
        let row = WorkTrailRowId::generate();
        let mut oversized = operation.clone();
        oversized["rowId"] = json!(row);
        match field {
            "tags" => oversized["tags"]
                .as_array_mut()
                .ok_or("tags")?
                .push(json!("extra")),
            "links" => oversized["links"]
                .as_array_mut()
                .ok_or("links")?
                .push(json!({"kind":"record","record":{"kind":"project","id":project}})),
            "path" => oversized["links"][0]["path"] = json!("x".repeat(1025)),
            "url" => {
                oversized["links"][0] =
                    json!({"kind":"url","url":format!("https://example.org/{}", "x".repeat(2048))})
            }
            _ => return Err("fixture field".into()),
        }
        let request = fixture.request(&project, oversized);
        let reply = fixture.api.call("work_trail_add", request.clone()).await?;
        assert_eq!(refusal_kind(&reply), Some("invalidShape"));
        assert_eq!(
            reply.pointer("/result/result/decision/reasons/0/reason/field"),
            Some(&json!(field))
        );
        let mut read = store.snapshot().await?;
        let high_water = read.high_water();
        assert!(read.record(&row.into()).await?.is_none());
        read.finish().await?;
        let retry = fixture.api.call("work_trail_add", request).await?;
        assert_eq!(
            retry.pointer("/result/result"),
            reply.pointer("/result/result")
        );
        let read = store.snapshot().await?;
        assert_eq!(
            read.high_water(),
            high_water,
            "replay appended another decision"
        );
        read.finish().await?;
    }
    drop(store);
    fixture.stop().await?;
    Ok(())
}
