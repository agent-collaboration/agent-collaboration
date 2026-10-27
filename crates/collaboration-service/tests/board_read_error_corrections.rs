//! Malformed board reads keep typed field diagnostics without exposing supplied values.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};

fn assert_field(response: &Value, kind: &str, field: &str) {
    assert_eq!(
        response.pointer("/error/data/kind"),
        Some(&json!(kind)),
        "{response}"
    );
    assert_eq!(
        response.pointer("/error/data/stage"),
        Some(&json!("validation"))
    );
    assert_eq!(
        response.pointer("/error/data/details/field"),
        Some(&json!(field)),
        "{response}"
    );
    assert!(!response.to_string().contains("BAD_PROJECT_INPUT"));
}

#[tokio::test]
async fn valid_read_actors_do_not_take_the_blame_for_malformed_read_fields() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    for reader in [
        json!({"kind":"anonymous"}),
        json!({"kind":"identified","reader":{"kind":"human","humanId":"reader"}}),
    ] {
        let response = fixture.api.call("feed_read", json!({"projectId":ProjectId::generate(),"reader":reader,"after":{"epoch":1,"position":-1},"limit":10,"includeRedacted":false})).await?;
        assert_field(&response, "invalidField", "after.position");
    }
    let response = fixture.api.call("feed_read", json!({"projectId":ProjectId::generate(),"reader":{"kind":"identified","reader":{"kind":"human","humanId":""}},"after":{"epoch":1,"position":0},"limit":10,"includeRedacted":false})).await?;
    assert_field(&response, "invalidIdentity", "reader.reader");
    fixture.stop().await
}

#[tokio::test]
async fn every_project_entity_read_uses_the_board_error_family() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for (show, list, kind) in [
        ("workstream_show", "workstream_list", "workstream"),
        ("task_show", "task_list", "task"),
        ("subtask_show", "subtask_list", "subtask"),
        ("role_show", "role_list", "roleAssignment"),
        ("fact_show", "fact_list", "fact"),
        ("intent_show", "intent_list", "notifyIntent"),
    ] {
        let response = fixture.api.call(show, json!({"projectId":"BAD_PROJECT_INPUT","record":{"kind":kind,"id":RecordId::generate()}})).await?;
        assert_field(&response, "invalidField", "projectId");
        let response = fixture
            .api
            .call(
                list,
                json!({"projectId":ProjectId::generate(),"page":{"limit":0}}),
            )
            .await?;
        assert_field(&response, "invalidField", "page.limit");
        let response = fixture.api.call(show, json!({"projectId":ProjectId::generate(),"record":{"kind":kind,"id":"BAD_PROJECT_INPUT"}})).await?;
        assert_field(&response, "invalidField", "record.id");
    }
    fixture.stop().await
}
