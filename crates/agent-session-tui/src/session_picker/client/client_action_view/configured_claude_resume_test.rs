//! The SSH process stands in for the remote executor; capture, wire decoding,
//! dispatch admission and refusal presentation stay real. No vendor is launched.
use crate::session_picker::PickerSourceContext;
use crate::session_picker::client::client_action_view::*;
use crate::session_picker::details::details_client_inventory::DetailsClientCapabilities;
use crate::session_picker::details::details_client_inventory::DetailsClientReadState;
use crate::session_picker::details::details_dialog_state::DetailsAction;
use crate::session_picker::details::details_dialog_state::SessionDetailsDialog;
use crate::session_picker::screen::fixtures::fixture_data::picker_request;
use agent_session_inventory::sessions::SessionPickerRecord;
use agent_session_inventory::sessions::router_connection_registry::RouterConnectionProfile;
use agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry;
use agent_session_inventory::sessions::source_client_action::execute_client_action;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeBinding;
use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeClaudeAction;
use agent_session_inventory::sessions::source_ssh_protocol::SourceOperation;
use agent_session_inventory::sessions::source_ssh_transport::read_source;
use collaboration_client::protocol::NativeSessionView;
use collaboration_client::protocol::ProviderSessionListParams;
use collaboration_client::protocol::SessionRef;
use serde_json::Value;
use serde_json::json;

const CHILD_TEST: &str = "session_picker::client::client_action_view::configured_claude_resume_test::configured_resume_child";
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

#[test]
fn old_native_ready_without_capabilities_round_trips() {
    let ready: agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability =
        serde_json::from_value(json!({"kind":"ready"})).unwrap();
    assert_eq!(
        serde_json::to_value(ready).unwrap(),
        json!({"kind":"ready"})
    );
}

fn profile() -> RouterConnectionProfile {
    RouterConnectionRegistry::parse(
        &json!({"version":1,"routers":[{
        "name":"Captured machine","connection":{"kind":"ssh","serviceId":SERVICE,
        "destination":"source.example.invalid","executable":"/fixed/agent-sessions",
        "serviceDirectory":"/private/source","clientProfile":"routerDebug"},
        "defaultRemoteCwd":"/source/project"}]})
        .to_string(),
    )
    .unwrap()
    .routers
    .remove(0)
}

fn source_page(scenario: &str) -> Value {
    let peer = if scenario == "stopped-live-peer" {
        json!({"name":"Older peer","status":"idle","startedAt":2,"updatedAt":3,
            "statusUpdatedAt":3,"kind":"interactive","entrypoint":"cli"})
    } else {
        Value::Null
    };
    json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},
        "observedAt":"2026-10-30T00:00:00Z","nextCursor":null,
        "nativeCoverage":{"peer":if scenario == "stopped-incomplete" {
            json!({"kind":"incomplete","unidentifiedJobs":1})
        } else {json!({"kind":"complete"})},"background":{"kind":"complete"}},
        "sessions":[{"origin":"claudeCodeBackground",
            "target":{"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},
                "sessionId":"00000000-0000-4000-8000-000000000042"},
            "jobHandle":"deadbeef","workingDirectory":"/source/project","startedAt":1,
            "state":{"kind":if scenario == "running" {"working"} else {"stopped"}},"peer":peer}]})
}

#[tokio::test]
async fn configured_old_executor_refuses_open_and_resume_but_guarded_executor_dispatches() {
    let root = tempfile::tempdir().unwrap();
    for guarded in [false, true] {
        let scenarios = if guarded {
            &["stopped", "wrong-service", "stale-epoch", "missing-member"][..]
        } else {
            &["running", "stopped-live-peer", "stopped-incomplete"][..]
        };
        for &scenario in scenarios {
            let mut native = json!({"kind":"ready"});
            if guarded {
                native["capabilities"] = json!(["guardedResume"]);
            }
            let mut inventory = json!({"kind":"clientInventory","nativeClaude":native,
                "inventory":{"serviceEpoch":EPOCH,"sequence":1,"endpoints":[{
                    "endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},
                    "label":"Source native Claude","channels":[],
                    "availability":{"state":"unavailable","observedAt":"2026-10-30T00:00:00Z","reason":"no managed channel"}}]}});
            match scenario {
                "wrong-service" => {
                    inventory["inventory"]["endpoints"][0]["endpoint"]["serviceId"] =
                        json!("00000000-0000-4000-8000-000000000009")
                }
                "stale-epoch" => {
                    inventory["inventory"]["serviceEpoch"] =
                        json!("00000000-0000-4000-8000-000000000009")
                }
                "missing-member" => inventory["inventory"]["endpoints"] = json!([]),
                _ => {}
            }
            node_test_fixture::write_node_fixture(
                &root.path().join("ssh"),
                "source-resume-guard.ts",
                &json!({"clientInventory":inventory,"page":source_page(scenario)}),
            )
            .unwrap();
            let count = root.path().join(format!("{scenario}-{guarded}.log"));
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
                    .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                    .env("SOURCE_SSH_COUNT", &count)
                    .env("SOURCE_EXPECT_GUARDED", if guarded { "1" } else { "0" })
                    .env("SOURCE_SCENARIO", scenario)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                output.status.success(),
                "{scenario}, guarded={guarded}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[tokio::test]
#[ignore = "isolated SSH PATH, invoked by parent"]
async fn configured_resume_child() {
    let profile = profile();
    let guarded = std::env::var("SOURCE_EXPECT_GUARDED").unwrap() == "1";
    let inventory = read_source(&profile, SourceOperation::ClientInventory)
        .await
        .unwrap();
    let SourceReply::ClientInventory { .. } = inventory else {
        panic!("inventory")
    };
    let endpoint =
        serde_json::from_value(json!({"serviceId":SERVICE,"endpointId":"claude-local"})).unwrap();
    let page = read_source(
        &profile,
        SourceOperation::ProviderSessions {
            params: ProviderSessionListParams {
                endpoint,
                view: NativeSessionView::Loaded,
                scope: collaboration_client::protocol::NativeSessionScope::Any,
                source: collaboration_client::protocol::NativeSessionSource::Interactive,
                query: None,
                page_size: 100,
                cursor: None,
            },
        },
    )
    .await
    .unwrap();
    let SourceReply::ProviderSessions { page } = page else {
        panic!("page")
    };
    let mut record = SessionPickerRecord::from_provider_summary(&page.sessions[0], "Source Claude");
    record.source_context = Some(PickerSourceContext::ConfiguredHosted(profile.clone()));
    assert_eq!(record.cwd.as_deref(), Some("/source/project"));
    if std::env::var("SOURCE_SCENARIO").unwrap() == "stopped-live-peer" {
        assert!(
            record
                .native_peer
                .as_ref()
                .unwrap()
                .working_directory
                .is_none()
        );
    }
    let mut dialog = SessionDetailsDialog::capture(&record);
    dialog.client_inventory = DetailsClientReadState::Ready(DetailsClientCapabilities::Native {
        source: SourceNativeBinding {
            service_id: profile.service_id.clone(),
            service_epoch: EPOCH.to_owned().try_into().unwrap(),
        },
    });
    let invalid_proof = matches!(
        std::env::var("SOURCE_SCENARIO").unwrap().as_str(),
        "wrong-service" | "stale-epoch" | "missing-member"
    );
    let mut model = SessionsPickerModel::new(picker_request(), 120);
    assert!(model.begin_details_client_action(&dialog, DetailsAction::Resume));
    let PickerClientAction::Queued(request) = model.client_action else {
        panic!("configured action")
    };
    let result = execute_client_action(None, None, None, None, request.clone()).await;
    if invalid_proof {
        assert!(
            result.is_err(),
            "a stale or misattributed proof must not dispatch: {result:?}"
        );
    } else if guarded {
        assert!(
            matches!(result, Ok(SourceReply::NativeClaude { .. })),
            "{result:?}"
        );
    } else {
        assert!(
            result.is_err(),
            "an older executor must receive no mutation: {result:?}"
        );
        assert_eq!(
            observation_lines(&request, &result),
            [
                "Captured machine runs an older agent-collaboration that could start a copy of this session. Update it (brew upgrade codex-router), then open again."
            ]
        );
    }
    let mutations = std::fs::read_to_string(std::env::var("SOURCE_SSH_COUNT").unwrap())
        .unwrap()
        .lines()
        .filter(|line| *line == "nativeClaude")
        .count();
    assert_eq!(mutations, if guarded && !invalid_proof { 1 } else { 0 });
    if !guarded {
        // Deliberate copies and New keep their existing dispatch policy.
        let target: SessionRef = serde_json::from_value(json!({"endpoint":{
            "serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":record.session_id}))
        .unwrap();
        for action in [
            SourceNativeClaudeAction::New {
                directory: "/source/project".to_owned().try_into().unwrap(),
                arguments: vec![],
            },
            SourceNativeClaudeAction::Fork {
                target,
                directory: "/source/project".to_owned().try_into().unwrap(),
                arguments: vec![],
            },
        ] {
            let result = execute_client_action(
                None,
                None,
                None,
                None,
                CapturedClientAction {
                    attempt: 1,
                    source: PickerSourceContext::ConfiguredHosted(profile.clone()),
                    service_id: profile.service_id.clone(),
                    effect: SessionClientEffect::Native {
                        service_epoch: EPOCH.to_owned().try_into().unwrap(),
                        action,
                    },
                },
            )
            .await;
            assert!(
                matches!(result, Ok(SourceReply::NativeClaude { .. })),
                "{result:?}"
            );
        }
    }
}
