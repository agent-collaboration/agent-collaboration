use super::*;

#[tokio::test]
async fn installation_show_transmits_the_observed_central_record_over_mcp() {
    use crate::api_test_harness::{ServedApi, api_config, test_identity};
    use board_sync_protocol::{FleetLinkRecord, FleetLinkRefusal, FleetLinkState};

    // Arrange: the retained failed-start record is independent of a running session.
    let peer = fleet_protocol::PeerId::new(uuid::Uuid::now_v7()).expect("UUIDv7 peer");
    let record = FleetLinkRecord {
        peer,
        endpoint: "wss://central.example.test/session".to_owned(),

        instance: 0,
        connection: 0,
        revision: 0,
        last_frame_at: None,
        state: FleetLinkState::Refused {
            reason: FleetLinkRefusal::IdentityInUseElsewhere,
            at: "2026-11-06T12:00:00Z".parse().expect("refusal timestamp"),
        },
        last_drop: None,
        last_failure: None,
    };
    let application = test_identity()
        .with_installation_peer(peer)
        .expect("installation peer")
        .with_fleet_link(board_fleet_sync::FleetLinkObservation::Failed(Box::new(
            record.clone(),
        )))
        .build();
    let directory = tempfile::tempdir().expect("service directory");
    let listener = ServedApi::tcp(&api_config(application, directory.path())).await;

    // Act: the real RMCP client crosses the listener and application boundary.
    let result = listener
        .call_tool("installation_show", serde_json::json!({}))
        .await;
    listener.stop().await;

    // Assert: returning identity alone or dropping refusal details must fail.
    assert_ne!(result.is_error, Some(true));
    let identity = result.structured_content.expect("installation identity");
    assert!(
        emitted_result_schema_tests::advertised_output_validator("installation_show")
            .is_valid(&identity),
        "advertised installation schema rejected the real MCP result: {identity}"
    );
    assert_eq!(identity["peerId"], peer.to_string());
    assert_eq!(identity["central"]["kind"], "available");
    assert_eq!(
        identity["central"]["record"],
        serde_json::to_value(record).expect("link record")
    );
}

#[tokio::test]
async fn installation_show_preserves_the_peer_when_its_central_observation_is_unavailable() {
    use crate::api_test_harness::{ServedApi, api_config, test_identity};
    // Arrange: identity is valid; the independently owned cache reader failed.
    let peer = fleet_protocol::PeerId::new(uuid::Uuid::now_v7()).expect("peer");
    let application = test_identity()
        .with_installation_peer(peer)
        .expect("identity")
        .with_fleet_link(board_fleet_sync::FleetLinkObservation::Unavailable {
            message: "controlled FleetCache read failure".to_owned(),
        })
        .build();
    let directory = tempfile::tempdir().expect("service directory");
    let listener = ServedApi::tcp(&api_config(application, directory.path())).await;

    // Act: real RMCP transport, application, identity and diagnostic read paths.
    let result = listener
        .call_tool("installation_show", serde_json::json!({}))
        .await;
    listener.stop().await;

    // Assert: re-enrollment can still obtain peer identity and the diagnostic is explicit.
    assert_ne!(
        result.is_error,
        Some(true),
        "link read failure hid installation identity"
    );
    let identity = result.structured_content.expect("installation identity");
    assert_eq!(identity["peerId"], peer.to_string());
    assert_eq!(identity["central"]["kind"], "unavailable");
    assert_eq!(
        identity["central"]["message"],
        "session store unavailable: controlled FleetCache read failure"
    );
}

#[tokio::test]
async fn installation_read_without_a_configured_peer_does_not_mint_an_identity() {
    let server = CollaborationMcpServer::catalog_only();
    assert!(server.application.installation_peer().is_none());
    let result = server
        .installation_show(super::super::Parameters(super::super::EmptyToolInput {}))
        .await;
    assert_eq!(result.is_error, Some(true));
    let error = result
        .structured_content
        .expect("installation unavailable result");
    assert_eq!(error["effect"], "none");
    assert!(!error.to_string().contains("peerId"));
    assert!(server.application.installation_peer().is_none());
}

#[test]
fn inspect_tool_exposes_native_rejection_message() {
    let message = "native thread is unreadable: fixture refusal";
    let rejection: Result<collaboration_protocol::NativeInspectResult, _> = Err(
        collaboration_service::collaboration_application::NativeSessionFailure::NativeRejected {
            stage: collaboration_service::collaboration_application::NativeSessionStage::Inspect,
            message: message.to_owned(),
            reason: collaboration_protocol::DeliveryRejectionReason::Unknown,
            next_action: collaboration_protocol::DeliveryNextAction::InspectTarget,
            native_code: Some(-32600),
        },
    );

    let result = super::super::application_result(rejection, OperationEffect::None);

    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured rejection");
    assert_eq!(structured["message"], message);
    assert_eq!(structured["data"]["message"], message);
    assert_eq!(structured["serviceKind"], "nativeRejected");
    assert_eq!(structured["effect"], "none");
}

#[tokio::test]
async fn inspect_without_a_codex_backend_is_a_typed_unavailable_error() {
    let server = CollaborationMcpServer::catalog_only();
    let target: collaboration_protocol::SessionRef =
        serde_json::from_value(fixture_session("codex-local", "requested-thread")).expect("target");

    let result = server
        .session_inspect(super::super::Parameters(
            collaboration_protocol::NativeInspectParams { target },
        ))
        .await;

    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured rejection");
    assert_eq!(structured["kind"], "rejected");
    assert_eq!(structured["effect"], "none");
}

#[test]
fn success_schema_branches_match_main_golden_snapshot() {
    // Set UPDATE_MAIN_SUCCESS_SCHEMA_SNAPSHOT=1 to refresh from the resolved tool catalog.
    let expected: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(include_str!("../snapshots/main_success_schemas.json"))
            .expect("main success schema snapshot");
    let server = CollaborationMcpServer::catalog_only();
    let actual = server
        .resolved_tools()
        .into_iter()
        .filter_map(|tool| {
            tool.output_schema.map(|schema| {
                let union = Value::Object((*schema).clone());
                let reference = union["anyOf"][0]["$ref"]
                    .as_str()
                    .expect("success branch reference");
                let definition_name = reference
                    .strip_prefix("#/$defs/")
                    .expect("local success definition");
                let mut success = union["$defs"][definition_name]
                    .as_object()
                    .expect("success definition object")
                    .clone();
                success.insert("$schema".to_owned(), union["$schema"].clone());
                success.insert(
                    "title".to_owned(),
                    Value::String(definition_name.to_owned()),
                );
                success
                    .entry("type".to_owned())
                    .or_insert_with(|| Value::String("object".to_owned()));
                let definitions = union["$defs"]
                    .as_object()
                    .expect("union definitions")
                    .iter()
                    .filter(|(name, _)| {
                        name.as_str() != definition_name && name.as_str() != "McpToolError"
                    })
                    .map(|(name, definition)| (name.clone(), definition.clone()))
                    .collect::<serde_json::Map<_, _>>();
                if !definitions.is_empty() {
                    success.insert("$defs".to_owned(), Value::Object(definitions));
                }
                (tool.name.to_string(), Value::Object(success))
            })
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    if std::env::var_os("UPDATE_MAIN_SUCCESS_SCHEMA_SNAPSHOT").is_some() {
        let entries = actual
            .iter()
            .enumerate()
            .map(|(index, (name, schema))| {
                let comma = if index + 1 < actual.len() { "," } else { "" };
                format!(
                    "{}:{}{}",
                    serde_json::to_string(name).expect("tool name JSON"),
                    serde_json::to_string(schema).expect("tool schema JSON"),
                    comma
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = format!("{{\n{entries}\n}}\n");
        let snapshot_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/mcp_server/snapshots/main_success_schemas.json");
        std::fs::write(snapshot_path, snapshot).expect("write resolved MCP success schemas");
        return;
    }
    assert_eq!(actual.len(), 165);
    assert_eq!(expected.len(), 165);
    for (name, success) in actual {
        assert_eq!(success, expected[&name], "{name} success schema drifted");
    }
}
