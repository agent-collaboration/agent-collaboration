//! Production inspector crosses source validation and actual Unix HTTP decoding.
//! The raw API stands outside this proof; no provider process or Host is started.
use super::*;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::source_inventory_request::PickerSourceContext;
use agent_session_picker::session_picker::details_client_inventory::DetailsClientQuery;
use collaboration_client::protocol::{ProviderKind, SessionRef};
use serde_json::{Value, json};
use std::time::Duration;

use agent_session_inventory::test_support::fake_api_support::{FakeCollaborationApi, FakeReply};

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

fn provider_endpoint(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Cursor => "cursor-local",
        ProviderKind::ClaudeCode => "claude-local",
    }
}

fn endpoint_inventory(load: bool, provider: ProviderKind) -> Value {
    json!({"serviceEpoch":EPOCH,"sequence":1,"endpoints":[{
        "endpoint":{"serviceId":SERVICE,"endpointId":provider_endpoint(provider)},
        "label":"Source Cursor","availability":{"state":"available","observedAt":"2026-10-18T00:00:00Z"},
        "channels":[{"kind":"externalProvider","transport":"stdioAcp","bindingId":"source-binding","bindingGeneration":7,
        "runtime":{"provider":provider,"runtimeName":"source-runtime"},
        "capabilities":[{"name":"load","status":if load {"supported"} else {"unsupported"},"evidence":"advertised"}]}]
    }]})
}

fn stored_summary(provider: ProviderKind) -> Value {
    json!({"origin":"hostedProvider","target":{"endpoint":{"serviceId":SERVICE,"endpointId":provider_endpoint(provider)},"sessionId":"source-stored-target"},
        "workingDirectory":"/source/saved","updatedAt":2,"state":"unloaded","requestedPolicy":{"access":"write-restricted"},
        "approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}})
}

fn inspection(state: &str, load: bool, resume: bool, provider: ProviderKind) -> Value {
    json!({"target":stored_summary(provider)["target"],"state":state,"history":"historyUnavailable",
        "capabilities":{"load":load,"resume":resume,"close":false,"list":false,"steer":false,"queue":null,"modes":false,
        "configOptions":false,"elicitation":false,"usage":false,"promptContent":{"image":false,"audio":false,"embeddedContext":false},"authStatus":{"kind":"notReported"}}})
}

async fn inspect_with_replies(
    provider: ProviderKind,
    replies: Vec<FakeReply>,
) -> (
    Result<DetailsClientCapabilities, SourceReadError>,
    Vec<Value>,
) {
    let home = tempfile::tempdir().unwrap();
    let mut api = FakeCollaborationApi::new(SERVICE, EPOCH).unwrap();
    let root = home.path().join(".codex-router");
    std::fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink(api.directory(), root.join("agent-communication")).unwrap();
    let peer = api.serve(replies);
    // No executable lookup or native preflight can reach the owner's environment.
    let context = crate::CliContext::new(vec![
        ("HOME".to_owned(), home.path().display().to_string()),
        ("PATH".to_owned(), String::new()),
    ]);
    let summary = serde_json::from_value(stored_summary(provider)).unwrap();
    let mut record = SessionPickerRecord::from_provider_summary(&summary, "Captured Cursor");
    record.source_context = Some(PickerSourceContext::DefaultHosted);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        production_inspector(context)(DetailsClientQuery {
            source: PickerSourceContext::DefaultHosted,
            record: Box::new(record),
        }),
    )
    .await
    .unwrap();
    assert!(
        !matches!(&result,Ok(DetailsClientCapabilities::Unavailable(message)) if message.contains("Claude uses its native background launcher")),
        "existing managed Claude must remain on its managed restore route: {result:?}"
    );
    let calls = tokio::time::timeout(Duration::from_secs(1), peer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    (result, calls)
}

fn replies(
    provider: ProviderKind,
    inventory: &Value,
    inspected: Value,
    final_inventory: &Value,
) -> Vec<FakeReply> {
    vec![
        FakeReply::Result(inventory.clone()),
        FakeReply::Result(inventory.clone()),
        FakeReply::Result(
            json!({"endpoint":stored_summary(provider)["target"]["endpoint"],"observedAt":"2026-10-18T00:00:00Z","sessions":[stored_summary(provider)],"nextCursor":null}),
        ),
        FakeReply::Result(inspected),
        FakeReply::Result(final_inventory.clone()),
    ]
}

#[tokio::test]
async fn production_details_inspector_uses_actual_source_capability_and_loaded_state() {
    for provider in [ProviderKind::Cursor, ProviderKind::ClaudeCode] {
        for (state, endpoint_load, session_load, resume, expected) in [
            (
                "unloaded",
                true,
                true,
                true,
                Some(ManagedRestoreActions::LoadAndResume),
            ),
            (
                "closed",
                true,
                true,
                false,
                Some(ManagedRestoreActions::Load),
            ),
            (
                "unloaded",
                false,
                true,
                true,
                Some(ManagedRestoreActions::Resume),
            ),
            (
                "unloaded",
                true,
                false,
                true,
                Some(ManagedRestoreActions::Resume),
            ),
            ("unloaded", false, true, false, None),
            ("idle", true, true, true, None),
            ("running", true, true, true, None),
        ] {
            let inventory = endpoint_inventory(endpoint_load, provider);
            let (result, calls) = inspect_with_replies(
                provider,
                replies(
                    provider,
                    &inventory,
                    inspection(state, session_load, resume, provider),
                    &inventory,
                ),
            )
            .await;
            let actual = result.unwrap();
            match expected {
                Some(expected) => assert!(
                    matches!(actual,DetailsClientCapabilities::Managed {actions,..} if actions==expected),
                    "{actual:?}"
                ),
                None => assert!(
                    matches!(actual, DetailsClientCapabilities::Unavailable(_)),
                    "{actual:?}"
                ),
            }
            assert_eq!(
                calls
                    .iter()
                    .map(|call| call["tool"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                [
                    "endpoints_list",
                    "endpoints_list",
                    "provider_sessions_list",
                    "provider_session_inspect",
                    "endpoints_list"
                ],
                "inspection must never create/load/resume"
            );
            assert_eq!(calls[2]["arguments"]["view"], "stored");
            assert_eq!(calls[2]["arguments"]["source"], "all");
            let target: SessionRef =
                serde_json::from_value(calls[3]["arguments"]["target"].clone()).unwrap();
            assert_eq!(String::from(target.session_id), "source-stored-target");
        }
    }
}

#[tokio::test]
async fn production_details_inspector_rejects_wrong_target_and_replaced_binding() {
    for provider in [ProviderKind::Cursor, ProviderKind::ClaudeCode] {
        let inventory = endpoint_inventory(true, provider);
        let mut wrong = inspection("unloaded", true, true, provider);
        wrong["target"]["sessionId"] = json!("another-session");
        let mut wrong_replies = replies(provider, &inventory, wrong, &inventory);
        wrong_replies.pop(); // Source rejects target before its final binding read.
        let (result, calls) = inspect_with_replies(provider, wrong_replies).await;
        assert!(
            result.is_err(),
            "another session's actions cannot be offered: {result:?}"
        );
        assert_eq!(calls.len(), 4);
        let mut replaced = inventory.clone();
        replaced["endpoints"][0]["channels"][0]["bindingGeneration"] = json!(8);
        let (result, calls) = inspect_with_replies(
            provider,
            replies(
                provider,
                &inventory,
                inspection("unloaded", true, true, provider),
                &replaced,
            ),
        )
        .await;
        assert!(
            result.is_err(),
            "replaced binding cannot authorize captured target: {result:?}"
        );
        assert_eq!(calls.len(), 5);
    }
}
