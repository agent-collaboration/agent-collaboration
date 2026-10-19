//! Native partition faults stay below HTTP decoding and the production continuation.
use super::*;
use crate::picker_runtime_status::{NativeLiveReadState, NativeLiveUnavailableReason};

const PRIVATE_SENTINEL: &str = "NATIVE_PARTITION_PRIVATE_SENTINEL";
#[derive(Clone, Copy, Debug)]
pub(crate) enum NativeScenario {
    Unavailable,
    InvalidParams,
    UnknownFailure,
    MalformedFailure,
    WrongErrorId,
    Empty,
    PrefixUnavailable,
    WrongOrigin,
    Duplicate,
    ChangedEpoch,
    ChannelLess,
    MissingEndpoint,
    WrongService,
    DuplicateEndpoint,
    PrefixHeld,
    PrefixHeldUnavailable,
}

pub(super) async fn reply(
    state: &Arc<PeerState>,
    request: &Value,
    scenario: NativeScenario,
) -> Value {
    let method = request
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap();
    let arguments = request.pointer("/params/arguments").unwrap();
    let endpoint = json!({"serviceId":state.service,"endpointId":"claude-local"});
    let native_codex = json!({"serviceId":state.service,"endpointId":"codex-local"});
    let loaded = arguments.get("view").and_then(Value::as_str) == Some("loaded");
    let cursor = arguments.get("cursor").and_then(Value::as_str);
    let loaded_calls = state
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, args)| {
            method == "provider_sessions_list"
                && args.get("view").and_then(Value::as_str) == Some("loaded")
        })
        .count();
    let result = match method {
        "endpoints_list" => {
            let mut endpoints = vec![
                json!({"endpoint":native_codex,"label":"Native Stored","availability":{"state":"unprobed"},
                "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock","schemaDigest":null,"generation":null}]}),
            ];
            if !matches!(scenario, NativeScenario::MissingEndpoint) {
                let channels = if matches!(scenario, NativeScenario::ChannelLess) {
                    json!([])
                } else {
                    json!([{"kind":"externalProvider","transport":"stdioAcp","bindingId":"managed-binding","bindingGeneration":1,
                        "runtime":{"provider":"claudeCode","runtimeName":"fixture"},"capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]}])
                };
                let mut description = json!({"endpoint":endpoint,"label":"Managed source","availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"},"channels":channels});
                if matches!(scenario, NativeScenario::WrongService) {
                    description["endpoint"]["serviceId"] =
                        json!("00000000-0000-4000-8000-000000000003");
                }
                endpoints.push(description.clone());
                if matches!(scenario, NativeScenario::DuplicateEndpoint) {
                    endpoints.push(description);
                }
            }
            json!({"serviceEpoch":if matches!(scenario,NativeScenario::ChangedEpoch)&&loaded_calls>0 {"00000000-0000-4000-8000-000000000004"}else {state.epoch.as_str()},"sequence":1,"endpoints":endpoints})
        }
        "sessions_list" => {
            json!({"endpoint":native_codex,"observedAt":"2026-10-18T00:00:00Z","sessions":[],"generation":null,"nextCursor":null})
        }
        "provider_sessions_list" => {
            assert_eq!(arguments.get("endpoint"), Some(&endpoint));
            assert!(
                arguments.get("query").is_none_or(Value::is_null),
                "no remote query reinterpretation"
            );
            if loaded {
                if matches!(
                    scenario,
                    NativeScenario::PrefixHeld | NativeScenario::PrefixHeldUnavailable
                ) && cursor.is_some()
                {
                    let _settled = HeldReplyGuard(Arc::clone(state));
                    state.native_started.add_permits(1);
                    state.release_page.acquire().await.unwrap().forget();
                }
                assert_eq!(
                    arguments.get("source").and_then(Value::as_str),
                    Some("interactive")
                );
                let failure = match scenario {
                    NativeScenario::Unavailable => Some((
                        -32050,
                        json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
                    )),
                    NativeScenario::PrefixUnavailable | NativeScenario::PrefixHeldUnavailable
                        if cursor.is_some() =>
                    {
                        Some((
                            -32050,
                            json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
                        ))
                    }
                    NativeScenario::InvalidParams => Some((
                        -32602,
                        json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
                    )),
                    NativeScenario::UnknownFailure => Some((
                        -32051,
                        json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
                    )),
                    NativeScenario::MalformedFailure => Some((
                        -32050,
                        json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL,"private":PRIVATE_SENTINEL}),
                    )),
                    NativeScenario::WrongErrorId => Some((
                        -32050,
                        json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
                    )),
                    _ => None,
                };
                if let Some((code, data)) = failure {
                    return json!({"jsonrpc":"2.0","id":if matches!(scenario,NativeScenario::WrongErrorId){json!(99)}else {request["id"].clone()},"error":{"code":code,"message":PRIVATE_SENTINEL,"data":data}});
                }
                let sessions = if matches!(scenario, NativeScenario::Empty) {
                    json!([])
                } else if matches!(scenario, NativeScenario::WrongOrigin) {
                    json!([managed_row(&endpoint)])
                } else {
                    json!([native_row(&endpoint)])
                };
                json!({"endpoint":endpoint,"observedAt":"2026-10-18T00:00:00Z","sessions":sessions,
                    "nextCursor":if cursor.is_none()&&matches!(scenario,NativeScenario::PrefixUnavailable|NativeScenario::Duplicate|NativeScenario::PrefixHeld|NativeScenario::PrefixHeldUnavailable){json!("native-next")}else {Value::Null}})
            } else {
                assert_eq!(
                    arguments.get("view").and_then(Value::as_str),
                    Some("stored")
                );
                assert_eq!(arguments.get("source").and_then(Value::as_str), Some("all"));
                json!({"endpoint":endpoint,"observedAt":"2026-10-18T00:00:00Z","sessions":[managed_row(&endpoint)],"nextCursor":null})
            }
        }
        _ => panic!("unexpected native partition tool {method}"),
    };
    json!({"jsonrpc":"2.0","id":request["id"],"result":{"content":[],"structuredContent":result,"isError":false}})
}

fn managed_row(endpoint: &Value) -> Value {
    json!({"origin":"hostedProvider","target":{"endpoint":endpoint,"sessionId":"equal-id"},"workingDirectory":"/source/managed","updatedAt":2000,"state":"idle","requestedPolicy":{"access":"write-restricted"},"approver":{"humanId":"saved-owner"},"createdBy":{"humanId":"creator"}})
}
fn native_row(endpoint: &Value) -> Value {
    json!({"origin":"claudeCodeInteractive","target":{"endpoint":endpoint,"sessionId":"equal-id"},"workingDirectory":"/source/native","name":"Native equal ID","status":"idle","startedAt":1,"updatedAt":2000,"statusUpdatedAt":2000,"kind":"interactive","entrypoint":"cli"})
}

#[tokio::test]
async fn configured_inventory_native_partitions_preserve_prefix_empty_and_qualified_origins() {
    for (scenario, expected_rows, expected_state) in [
        (
            NativeScenario::Unavailable,
            1,
            NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (NativeScenario::Empty, 1, NativeLiveReadState::Complete),
        (
            NativeScenario::PrefixUnavailable,
            2,
            NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (
            NativeScenario::ChannelLess,
            1,
            NativeLiveReadState::Complete,
        ),
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let snapshot = super::super::configured_inventory_read_tests::read_complete_source(
            peer.loader(),
            peer.request(),
        )
        .await
        .into_snapshot()
        .unwrap();
        assert_eq!(snapshot.records.len(), expected_rows, "{scenario:?}");
        assert_eq!(
            snapshot.native_live_coverage.first().unwrap().state,
            expected_state,
            "{scenario:?}"
        );
        assert!(!format!("{snapshot:?}").contains(PRIVATE_SENTINEL));
        if expected_rows == 2 {
            assert!(snapshot.records.iter().any(|row| matches!(
                row.identity,
                SessionPickerIdentity::HostedProvider(_)
            ) && row.cwd.as_deref()
                == Some("/source/managed")));
            assert!(snapshot.records.iter().any(|row| matches!(
                row.identity,
                SessionPickerIdentity::NativeClaude(_)
            ) && row.cwd.as_deref()
                == Some("/source/native")));
        }
        if matches!(scenario, NativeScenario::ChannelLess) {
            assert_eq!(
                snapshot.records.first().unwrap().client_provider_family(),
                crate::presentation::session_picker::SessionProviderFamily::External(
                    collaboration_client::protocol::ProviderKind::ClaudeCode
                ),
                "typed native origin must stay in Claude family when ACP is absent"
            );
            let calls = peer.state.calls.lock().unwrap();
            assert!(
                !calls
                    .iter()
                    .any(|(name, args)| name == "provider_sessions_list"
                        && args.get("view").and_then(Value::as_str) == Some("stored")),
                "native-only membership must not require managed store"
            );
        }
        peer.finish().await;
    }
}

#[tokio::test]
async fn configured_inventory_native_invalid_reply_never_preserves_a_partial_success() {
    for (scenario, reason) in [
        (
            NativeScenario::InvalidParams,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::UnknownFailure,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::MalformedFailure,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::WrongErrorId,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::WrongOrigin,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::Duplicate,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::ChangedEpoch,
            SourceInventoryRejection::StaleSnapshot,
        ),
        (
            NativeScenario::WrongService,
            SourceInventoryRejection::InvalidInventory,
        ),
        (
            NativeScenario::DuplicateEndpoint,
            SourceInventoryRejection::InvalidInventory,
        ),
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let rejection = super::super::configured_inventory_read_tests::read_complete_source(
            peer.loader(),
            peer.request(),
        )
        .await
        .into_snapshot()
        .unwrap_err();
        assert_eq!(rejection, reason, "{scenario:?}");
        assert!(!rejection.to_string().contains(PRIVATE_SENTINEL));
        peer.finish().await;
    }
    let peer = ReadPeer::start(ReplyScenario::NativePartition(
        NativeScenario::MissingEndpoint,
    ))
    .await;
    let snapshot = super::super::configured_inventory_read_tests::read_complete_source(
        peer.loader(),
        peer.request(),
    )
    .await
    .into_snapshot()
    .unwrap();
    assert!(snapshot.records.is_empty() && snapshot.native_live_coverage.is_empty());
    assert_eq!(
        peer.page_calls(),
        1,
        "only the actual Codex Stored partition exists"
    );
    peer.finish().await;
}

#[tokio::test]
async fn configured_inventory_native_timeout_keeps_completed_stored_and_live_prefix() {
    let peer = ReadPeer::start(ReplyScenario::NativePartition(NativeScenario::PrefixHeld)).await;
    let loader = peer.loader();
    let request = peer.request();
    let read = tokio::spawn(async move {
        super::super::configured_inventory_read_tests::read_complete_source(loader, request).await
    });
    peer.wait_for_held_page().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let snapshot = read
        .await
        .unwrap()
        .into_snapshot()
        .expect("extra live timeout must not erase either validated partition");
    tokio::time::resume();
    assert_eq!(snapshot.records.len(), 2);
    assert_eq!(
        snapshot.native_live_coverage.first().unwrap().state,
        NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::TimedOut)
    );
    assert!(
        snapshot
            .records
            .iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::HostedProvider(_)))
    );
    assert!(
        snapshot
            .records
            .iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_)))
    );
    peer.release_held_page().await;
    peer.finish().await;
}
