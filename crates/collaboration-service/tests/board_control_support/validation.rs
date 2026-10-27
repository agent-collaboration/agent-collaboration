//! Retained malformed-input field, bound and privacy assertions after API-6.
use super::board_fixture::{BoardFixture, TestError};
use message_board::{PostId, RequestId};
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove exact field/privacy controls; Result propagates fixture errors."
)]
async fn malformed_board_requests_return_safe_specific_failures() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let valid_id = PostId::generate();
    let actor = json!({"kind":"human","humanId":"reader"});
    let private_value = "PRIVATE_REQUEST_VALUE_MUST_NOT_ECHO";
    let header = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":actor});
    let mut cases = vec![
        (
            "inbox_fetch",
            json!({"protocolVersion":1,"reader":actor,"readMode":"unread","initializations":[],"page":{}}),
            "invalidField",
            "scope",
            "required",
        ),
        (
            "inbox_fetch",
            json!({"protocolVersion":1,"reader":actor,"readMode":"latest","initializations":[],"page":{"cursor":"opaque-continuation"}}),
            "invalidField",
            "scope",
            "required",
        ),
        (
            "workspace_create",
            json!({"workspaceId":valid_id,"name":"Project","description":"","actor":{"kind":"futureSecret","private":private_value}}),
            "invalidIdentity",
            "actor",
            "4096",
        ),
        (
            "inbox_fetch",
            json!({"protocolVersion":1,"scope":{"kind":"project","projectId":valid_id},"reader":null,"initializations":[],"page":{"limit":50,"cursor":null}}),
            "invalidIdentity",
            "reader",
            "4096",
        ),
        (
            "workspace_create",
            json!({"workspaceId":valid_id,"name":"Project","description":"","actingFor":{"kind":"session","session":{"private":private_value}}}),
            "invalidIdentity",
            "actingFor",
            "4096",
        ),
        (
            "project_create",
            json!({"projectId":valid_id,"boardId":valid_id,"title":"   "}),
            "invalidProjectName",
            "title",
            "256",
        ),
        (
            "project_show",
            json!({"projectId":"not-a-uuid"}),
            "invalidField",
            "projectId",
            "UUIDv7",
        ),
        (
            "workspace_create",
            json!({"workspaceId":valid_id,"name":"Project","description":"x".repeat(16_385)}),
            "invalidField",
            "description",
            "16384",
        ),
        (
            "workspace_create",
            json!({"workspaceId":valid_id,"name":"n".repeat(257),"description":""}),
            "invalidField",
            "name",
            "256",
        ),
        (
            "thread_post",
            json!({"postId":valid_id,"placement":{"kind":"thread","thread":{"thread":valid_id}},"text":"t".repeat(65_537),"references":[],"addressees":[]}),
            "invalidField",
            "text",
            "65536",
        ),
        (
            "workspace_list",
            json!({"repository":null,"page":{"limit":101,"cursor":null}}),
            "invalidField",
            "page.limit",
            "100",
        ),
        (
            "post_list",
            json!({"scope":{"kind":"project","projectId":valid_id},"selection":{"kind":"afterPosition","after":{"kind":"project","cursor":{"epoch":1,"position":9_223_372_036_854_775_808_u64}}},"page":{"limit":50,"cursor":null}}),
            "invalidField",
            "selection.after.cursor.position",
            "9223372036854775807",
        ),
        (
            "thread_post",
            json!({"postId":valid_id,"placement":{"kind":"privateKind","private":private_value},"text":"message","references":[],"addressees":[]}),
            "invalidField",
            "placement",
            "existing thread",
        ),
        (
            "project_show",
            json!({"projectId":valid_id,"privateField":private_value}),
            "invalidField",
            "request",
            "camelCase",
        ),
    ];
    assert_eq!(cases.len(), 14);
    for (tool, fields, kind, field, requirement) in &mut cases {
        if matches!(*tool, "workspace_create" | "project_create" | "thread_post") {
            for (key, value) in header.as_object().ok_or("header")? {
                fields
                    .as_object_mut()
                    .ok_or("fields")?
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        let response = fixture.api.call(tool, fields.clone()).await?;
        assert_eq!(
            response.pointer("/error/data/kind"),
            Some(&json!(kind)),
            "{tool}: {response}"
        );
        assert_eq!(
            response.pointer("/error/data/details/field"),
            Some(&json!(field)),
            "{tool}: {response}"
        );
        assert!(
            response
                .pointer("/error/data/details/requirement")
                .and_then(Value::as_str)
                .is_some_and(|value| value.contains(*requirement)),
            "{tool}: {response}"
        );
        let encoded = response.to_string();
        for marker in [private_value, "privateField", "privateKind"] {
            assert!(
                !encoded.contains(marker),
                "validation echoed supplied value/key/variant: {response}"
            );
        }
    }
    fixture.stop().await
}
