#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Locator validation and bounded repository pages through the real native fleet path.
use message_board::public_replies::{RequestDecision, RequestResult};
use message_board::*;
use serde_json::json;
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
use board_fixture::{BoardFixture, TestError};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn oversized_repository_locators_return_field_and_numeric_bound() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for (repository, field) in [
        (
            json!({"kind":"origin","normalizedOrigin":format!("github.com/{}","x".repeat(455_000))}),
            "repository.normalizedOrigin",
        ),
        (
            json!({"kind":"local","serviceId":"00000000-0000-4000-8000-000000000001","commonDirectory":format!("/{}","x".repeat(500_000))}),
            "repository.commonDirectory",
        ),
    ] {
        let mut request = fixture.header();
        request["workspaceId"] = json!(fixture.workspace);
        request["repository"] = repository;
        let response = fixture.api.call("repository_attach", request).await?;
        assert_eq!(
            response.pointer("/error/data/kind"),
            Some(&json!("invalidField")),
            "{response}"
        );
        assert_eq!(
            response.pointer("/error/data/details/field"),
            Some(&json!(field)),
            "{response}"
        );
        assert!(
            response
                .pointer("/error/data/details/requirement")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value.contains("4096")),
            "{response}"
        );
        assert!(
            response.to_string().len() < 1_048_576,
            "validation response exceeds frame"
        );
    }
    let client = collaboration_client::CollaborationClient::connect(
        fixture._directory.path(),
        "repository-boundary-test",
        "1",
    )
    .await?;
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    for marker in ['a', 'b'] {
        let reply = client
            .repository_attach(RepositoryAttachRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                workspace_id: fixture.workspace.clone(),
                repository: RepositoryRef::Local {
                    service_id: ServiceId::try_from(
                        "00000000-0000-4000-8000-000000000001".to_owned(),
                    )?,
                    common_directory: CommonDirectory::try_from(format!(
                        "/{marker}{}",
                        "\\\"".repeat(2_047)
                    ))?,
                },
                actor: actor.clone(),
                acting_for: None,
            })
            .await?;
        assert!(
            matches!(
                reply.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{reply:?}"
        );
    }
    let first = client
        .repository_list(RepositoryListRequest {
            workspace_id: fixture.workspace.clone(),
            page: PageRequest {
                limit: PageLimit::try_from(1)?,
                cursor: None,
            },
        })
        .await?;
    assert_eq!(first.page.records.len(), 1);
    assert!(
        serde_json::to_vec(&first)?.len() < 1_048_576,
        "first max-bound page exceeds frame"
    );
    assert!(
        first.page.next_cursor.is_some(),
        "second repository requires continuation"
    );
    let second = client
        .repository_list(RepositoryListRequest {
            workspace_id: fixture.workspace.clone(),
            page: PageRequest {
                limit: PageLimit::try_from(1)?,
                cursor: first.page.next_cursor,
            },
        })
        .await?;
    assert_eq!(second.page.records.len(), 1);
    assert!(
        second.page.next_cursor.is_none(),
        "second max-bound page must finish"
    );
    assert!(
        serde_json::to_vec(&second)?.len() < 1_048_576,
        "second max-bound page exceeds frame"
    );
    fixture.stop().await?;
    Ok(())
}
