//! Full access is refused until session authentication exists (D1 R19,
//! V34): a start or a mode change into Claude's bypassPermissions is refused
//! through the real client → API → supervisor path, naming the reason, and
//! nothing reaches the agent.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use crate::provider_conversation_submission::submit_provider_create;
use crate::provider_send_behaviour::{ComposedHost, recorded};
use codex_router_host::ExternalProviderLaunchBinding;
use collaboration_protocol::{
    ConversationCreateRequest, OperationId, ProviderRequestedPolicy, ProviderSettingName,
    ProviderSettingsFailureKind, ProviderSettingsSetRequest, ProviderWorkingDirectory,
    RouterAccess,
};
use std::path::Path;

const REFUSAL: &str = "session authentication";

fn claude_recording(receipt: &Path) -> ExternalProviderLaunchBinding {
    ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "send-race".to_owned(),
            receipt.display().to_string(),
        ],
    )
    .expect("Claude binding")
}

/// Oracle: D1 R19 and V34, "start ... with each full-access mode: refused,
/// naming session authentication; nothing starts": a Claude start naming
/// bypassPermissions is refused, and the agent never sees session/new.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_in_bypass_permissions_is_refused_and_nothing_starts() {
    // Arrange
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(claude_recording(&receipt), "claude-local").await;
    let owner = ComposedHost::human("full-access-owner");

    // Act
    let refused = submit_provider_create(
        &host.client,
        ConversationCreateRequest {
            settings: Some(collaboration_protocol::ProviderRequestedSettings {
                mode: Some("bypassPermissions".into()),
                ..collaboration_protocol::ProviderRequestedSettings::default()
            }),
            operation_id: OperationId::generate(),
            endpoint: host.endpoint.clone(),
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(
                host.root.path().display().to_string(),
            )
            .expect("working directory"),
            created_by: owner.clone(),
            approver: owner,
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await;
    let session_news = recorded(&receipt, "session/new");
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    let refusal = format!("{refused:?}");
    assert!(refused.is_err(), "the start is refused: {refusal}");
    assert!(
        refusal.contains("authenticationRequired") && refusal.contains(REFUSAL),
        "the refusal names session authentication: {refusal}"
    );
    assert_eq!(session_news, 0, "nothing starts at the agent");
}

/// Oracle: D1 R19 and V34, "settings set with each full-access mode: refused,
/// naming session authentication; nothing changes": a running Claude session
/// asked to change its mode to bypassPermissions is refused, and the agent
/// is never asked to change its mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mode_change_to_bypass_permissions_is_refused_and_nothing_changes() {
    // Arrange
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(claude_recording(&receipt), "claude-local").await;
    let owner = ComposedHost::human("full-access-owner");
    let target = host.create(&owner).await;
    let mode_changes_before =
        recorded(&receipt, "session/set_mode") + recorded(&receipt, "session/set_config_option");

    // Act
    let refused = host
        .client
        .set_provider_conversation_setting(ProviderSettingsSetRequest {
            target,
            actor: owner,
            setting: ProviderSettingName::Mode,
            value: "bypassPermissions".to_owned(),
        })
        .await;
    let mode_changes_after =
        recorded(&receipt, "session/set_mode") + recorded(&receipt, "session/set_config_option");
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    let failure: collaboration_protocol::ProviderSettingsFailure = match refused {
        Err(collaboration_client::ClientError::Rejected {
            data: Some(data), ..
        }) => serde_json::from_value(data).expect("a typed settings failure"),
        other => panic!("the mode change is refused: {other:?}"),
    };
    assert_eq!(failure.kind, ProviderSettingsFailureKind::InvalidSetting);
    assert!(
        String::from(failure.message.clone()).contains(REFUSAL),
        "the refusal names session authentication: {failure:?}"
    );
    assert_eq!(
        mode_changes_after, mode_changes_before,
        "the agent is never asked to change its mode"
    );
}
