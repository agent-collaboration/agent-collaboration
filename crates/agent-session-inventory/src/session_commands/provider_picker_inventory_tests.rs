//! Provider partitions and retained Stored metadata through actual read boundaries.
use super::*;

fn native_catalogue_page(sessions: Value, stored: bool) -> Value {
    json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"observedAt":"2026-09-07T00:00:00Z","sessions":sessions,"nextCursor":null,
        "nativeCoverage":{"peer":{"kind":if stored {"notRequested"}else {"complete"}},"background":{"kind":"complete"}}})
}

#[tokio::test]
async fn provider_inventory_reaches_picker_as_a_qualified_read_only_row() {
    let provider_page = json!({
        "endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},
        "observedAt":"2026-09-07T00:00:00Z",
        "sessions":[{
            "origin":"hostedProvider",
            "target":{"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"first"},
            "workingDirectory":"/repo","updatedAt":3,"state":"requiresAction","requestedPolicy":{"access":"write-restricted"},
            "approver":{"kind":"human","humanId":"owner"},
            "createdBy":{"endpoint":endpoint(),"sessionId":"creator"}
        }],"nextCursor":null
    });
    let (mut client, client_done, peer) = connect_fixture(vec![
        ("endpoints_list", inventory_with_provider()),
        ("provider_sessions_list", provider_page),
        (
            "provider_sessions_list",
            native_catalogue_page(json!([]), true),
        ),
        (
            "provider_sessions_list",
            native_catalogue_page(json!([]), false),
        ),
        ("endpoints_list", inventory_with_provider()),
    ])
    .await;
    let (native_endpoint, rows, _) = load_provider_records(&mut client)
        .await
        .expect("provider rows");
    let _completed = client_done.send(());
    let requests = peer.await.expect("peer");
    assert_eq!(
        native_endpoint.expect("native endpoint"),
        serde_json::from_value(endpoint()).unwrap()
    );
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].identity,
        SessionPickerIdentity::HostedProvider(_)
    ));
    assert!(rows[0].title.contains("Claude fixture"));
    assert_eq!(rows[0].runtime_status, PickerRuntimeStatus::Blocked);
    assert_eq!(requests[1]["arguments"]["source"], "all");
}

#[tokio::test]
async fn provider_inventory_adds_loaded_native_claude_without_collapsing_managed_equal_ids() {
    let target = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"});
    let managed = json!({"origin":"hostedProvider","target":target,
        "workingDirectory":"/managed","updatedAt":3,"state":"idle",
        "requestedPolicy":{"access":"write-restricted"},
        "approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}});
    let native = json!({"origin":"claudeCodeInteractive","target":target,
        "workingDirectory":"/native","name":"Live native Claude","status":"idle",
        "startedAt":1,"updatedAt":3,"statusUpdatedAt":3,"kind":"interactive","entrypoint":"cli"});
    let provider_page = |sessions: Value| {
        json!({"endpoint":target["endpoint"],
        "observedAt":"2026-09-07T00:00:00Z","sessions":sessions,"nextCursor":null})
    };
    let (mut client, client_done, peer) = connect_fixture(vec![
        ("endpoints_list", inventory_with_provider()),
        (
            "provider_sessions_list",
            provider_page(json!([managed.clone()])),
        ),
        (
            "provider_sessions_list",
            native_catalogue_page(json!([]), true),
        ),
        (
            "provider_sessions_list",
            native_catalogue_page(json!([native]), false),
        ),
        ("endpoints_list", inventory_with_provider()),
    ])
    .await;
    let (_, rows, coverage) = load_provider_records(&mut client)
        .await
        .expect("qualified provider rows");
    assert_eq!(
        rows.len(),
        2,
        "Stored managed and Loaded native origins both remain visible: {rows:?}"
    );
    assert!(rows.iter().any(|row| matches!(
        row.identity,
        SessionPickerIdentity::HostedProvider(_)
    ) && row.cwd.as_deref() == Some("/managed")));
    assert!(rows.iter().any(
        |row| matches!(row.identity, SessionPickerIdentity::NativeClaude(_))
            && row.cwd.as_deref() == Some("/native")
    ));
    let _completed = client_done.send(());
    let requests = peer.await.unwrap();
    assert_eq!(requests[1]["arguments"]["view"], "stored");
    assert_eq!(requests[2]["arguments"]["view"], "stored");
    assert_eq!(requests[3]["arguments"]["view"], "loaded");
    assert_eq!(requests[2]["arguments"]["source"], "interactive");
    assert_eq!(coverage.len(), 2);
    assert_eq!(
        coverage[0].state,
        crate::picker_runtime_status::NativeLiveReadState::Complete
    );
    assert_eq!(
        requests.len(),
        5,
        "only read-only inventory requests and binding revalidation"
    );
}

#[tokio::test]
async fn hosted_refresh_keeps_equal_provider_and_codex_ids_as_two_rows() {
    use collaboration_client::protocol::{
        EndpointDescription, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
        SessionRef,
    };
    use collaboration_runtime::ProviderOperationStore;
    use collaboration_runtime::ProviderSessionRecord;
    use collaboration_service::ApplicationFixture;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let root = tempfile::Builder::new()
        .prefix("provider-picker-")
        .tempdir_in("/tmp")
        .expect("service root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private root");
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"shared-id"
    }))
    .expect("provider target");
    let creator: SessionRef = serde_json::from_value(json!({
        "endpoint":endpoint(),"sessionId":"creator"
    }))
    .expect("creator");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/repo/project-a".to_owned())
                .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: creator.clone().into(),
            approver: creator.into(),
            updated_at_ms: 3_000,
        })
        .await
        .expect("provider record");
    // This service deliberately has no native backend. Keep Stored attribution
    // qualified without advertising a runtime connection the fixture cannot serve.
    let mut descriptions_json = inventory_with_provider();
    descriptions_json["endpoints"][0]["availability"] = json!({
        "state":"unavailable","observedAt":"2026-09-07T00:00:00Z",
        "reason":"Fixture has no native backend"
    });
    let descriptions: Vec<EndpointDescription> = descriptions_json["endpoints"]
        .as_array()
        .expect("descriptions")
        .iter()
        .cloned()
        .map(|entry| serde_json::from_value(entry).expect("description"))
        .collect();
    let identity = ApplicationFixture::new(SERVICE, EPOCH)
        .expect("identity")
        .with_endpoints(descriptions)
        .expect("endpoints")
        .with_provider_operation_store(store);
    let served = collaboration_mcp::test_support::ServedCollaborationApi::start(
        root.path(),
        (identity).build(),
    )
    .expect("serve collaboration API");
    let codex = runtime_record(
        "shared-id",
        "Codex row",
        "/repo/project-a",
        &json!({"source":"cli"}),
    );
    let rows = PickerRuntimeInventory::default()
        .refresh(Some(root.path()), vec![codex], false)
        .await
        .unwrap()
        .records;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::HostedCodex(_)))
    );
    assert!(
        rows.iter()
            .any(|row| matches!(row.identity, SessionPickerIdentity::HostedProvider(_)))
    );
    served.stop().await.expect("collaboration API stops");
}

#[test]
fn equal_provider_session_ids_keep_managed_and_native_claude_origins_distinct() {
    let target = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"same-visible-id"});
    let managed: collaboration_client::protocol::ProviderSessionSummary =
        serde_json::from_value(json!({
            "origin":"hostedProvider","target":target,"workingDirectory":"/managed/project",
            "updatedAt":3,"state":"idle","requestedPolicy":{"access":"write-restricted"},
            "approver":{"humanId":"saved-approver"},"createdBy":{"humanId":"creator"}
        }))
        .expect("managed summary");
    let native: collaboration_client::protocol::ProviderSessionSummary =
        serde_json::from_value(json!({
            "origin":"claudeCodeInteractive","target":target,"workingDirectory":"/native/project",
            "name":"Native fixture","status":"idle","startedAt":1,"updatedAt":3,
            "statusUpdatedAt":3,"kind":"interactive","entrypoint":"cli"
        }))
        .expect("native summary");
    let managed_row = SessionPickerRecord::from_provider_summary(&managed, "Claude");
    let native_row = SessionPickerRecord::from_provider_summary(&native, "Claude");
    assert_eq!(managed_row.session_id, native_row.session_id);
    assert_ne!(
        managed_row.identity, native_row.identity,
        "native and managed actions must not share a picker identity even with equal endpoint and visible ID"
    );
    let identities = std::collections::BTreeSet::from([managed_row.identity, native_row.identity]);
    assert_eq!(
        identities.len(),
        2,
        "origin-qualified refresh deduplication must retain both records"
    );
}

#[tokio::test]
async fn local_native_availability_keeps_stored_when_final_probe_is_unavailable_but_not_invalid() {
    use crate::picker_runtime_status::{NativeLiveReadState, NativeLiveUnavailableReason};
    let target = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"retained-managed"});
    let stored = json!({"endpoint":target["endpoint"],"observedAt":"2026-10-18T00:00:00Z","nextCursor":null,"sessions":[{
        "origin":"hostedProvider","target":target,"workingDirectory":"/retained","updatedAt":2,"state":"idle","requestedPolicy":{"access":"write-restricted"},"approver":{"humanId":"owner"},"createdBy":{"humanId":"creator"}}]});
    let unavailable = json!({"code":-32050,"message":"PRIVATE_LOCAL_SENTINEL","data":{"kind":"unavailable","stage":"discovery","message":"PRIVATE_LOCAL_SENTINEL"}});
    for valid_availability in [true, false] {
        let mut api = FakeCollaborationApi::new(SERVICE, EPOCH).unwrap();
        let final_reply = if valid_availability {
            unavailable.clone()
        } else {
            json!({"code":-32602,"message":"PRIVATE_LOCAL_SENTINEL","data":{"private":"PRIVATE_LOCAL_SENTINEL"}})
        };
        let (client_done, peer) = api.serve_then_watch(vec![
            FakeReply::Result(inventory_with_provider()),
            FakeReply::Result(stored.clone()),
            FakeReply::Error(unavailable.clone()),
            FakeReply::Error(unavailable.clone()),
            FakeReply::Error(final_reply),
        ]);
        let mut client = CollaborationClient::connect(api.directory(), "local-partial-proof", "1")
            .await
            .unwrap();
        let result = load_provider_records(&mut client).await;
        let _completed = client_done.send(());
        let transcript = tokio::time::timeout(std::time::Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(transcript.calls.len(), 5);
        assert!(transcript.further_call.is_none());
        if valid_availability {
            let (_, records, coverage) = result
                .expect("unavailable extra native revalidation must retain qualified Stored rows");
            assert_eq!(records.len(), 1);
            assert_eq!(records.first().unwrap().session_id, "retained-managed");
            assert_eq!(
                coverage.first().unwrap().state,
                NativeLiveReadState::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable)
            );
            assert!(!format!("{coverage:?}").contains("PRIVATE_LOCAL_SENTINEL"));
        } else {
            let rejection = result.unwrap_err();
            assert!(!format!("{rejection:?}").contains("PRIVATE_LOCAL_SENTINEL"));
        }
    }
}
