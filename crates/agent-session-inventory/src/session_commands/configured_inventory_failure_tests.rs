//! Actual HTTP JSON-RPC/tool envelopes, before any loader classification.
use super::*;
use serde_json::{Value, json};

const PRIVATE_SENTINEL: &str = "PRIVATE_INVENTORY_FAILURE_SENTINEL_DO_NOT_PRESENT";

#[tokio::test]
async fn configured_http_proxy_failure_is_unavailable_not_invalid_inventory() {
    use crate::picker_runtime_status::NativeLiveUnavailableReason;
    use crate::source_inventory_request::SourceInventoryRejection;
    use crate::test_support::{
        recorded_api_listener::RecordedApiListener,
        source_fault_fixture::{ReadPeer, ReplyScenario, native_partitions::NativeScenario},
    };
    use tokio_util::sync::CancellationToken;

    let peer = ReadPeer::start(ReplyScenario::NativePartition(NativeScenario::PrefixHeld)).await;
    let PickerSourceContext::ConfiguredHosted(upstream_profile) = peer.request().source_context
    else {
        panic!("configured upstream fixture")
    };
    let RouterConnectionTransport::Remote { mcp_address, .. } = upstream_profile.transport else {
        panic!("HTTP upstream fixture")
    };
    let shutdown = CancellationToken::new();
    let proxy = RecordedApiListener::start(mcp_address.as_str().to_owned(), shutdown.clone())
        .await
        .unwrap();
    let profile = profile_for_address(proxy.url.as_ref().unwrap());
    let client = HttpConfiguredInventoryClient::new(&profile).unwrap();
    let mut request = ProviderSessionListParams {
        endpoint: serde_json::from_value(
            json!({"serviceId":profile.service_id,"endpointId":"claude-local"}),
        )
        .unwrap(),
        view: collaboration_client::protocol::NativeSessionView::Loaded,
        scope: collaboration_client::protocol::NativeSessionScope::Any,
        source: collaboration_client::protocol::NativeSessionSource::Interactive,
        query: None,
        page_size: 100,
        cursor: None,
    };
    let first = client.provider_page(&request).await.unwrap();
    request.cursor = Some(first.next_cursor.expect("fixture exposes a held next page"));
    let reading = tokio::spawn(async move { client.provider_page(&request).await });
    peer.wait_for_held_page().await;
    shutdown.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), reading)
        .await
        .unwrap()
        .unwrap();
    peer.release_held_page().await;
    proxy.finish().await.unwrap();
    peer.finish().await;

    let failure = result.expect_err("the held upstream page must fail through the proxy");
    assert_eq!(
        failure,
        InventoryReadFailure::Unavailable(NativeLiveUnavailableReason::TransportUnavailable),
        "an HTTP upstream failure is availability, never invalid source data"
    );
    assert_eq!(
        failure.into_rejection(),
        SourceInventoryRejection::SourceUnavailable
    );
}

#[tokio::test]
async fn configured_http_server_failures_are_distinct_from_invalid_replies() {
    use crate::picker_runtime_status::NativeLiveUnavailableReason;

    for status in [500_u16, 502, 503, 504, 599, 400, 404, 200] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let profile =
            profile_for_address(&format!("http://{}/mcp", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            let router = axum::Router::new().route(
                "/mcp",
                axum::routing::post(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        "invalid reply",
                    )
                }),
            );
            axum::serve(listener, router).await.unwrap();
        });
        let request = ProviderSessionListParams {
            endpoint: serde_json::from_value(
                json!({"serviceId":profile.service_id,"endpointId":"claude-local"}),
            )
            .unwrap(),
            view: collaboration_client::protocol::NativeSessionView::Loaded,
            scope: collaboration_client::protocol::NativeSessionScope::Any,
            source: collaboration_client::protocol::NativeSessionSource::Interactive,
            query: None,
            page_size: 100,
            cursor: None,
        };
        let failure = HttpConfiguredInventoryClient::new(&profile)
            .unwrap()
            .provider_page(&request)
            .await
            .unwrap_err();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        let expected = match status {
            500..=599 => {
                InventoryReadFailure::Unavailable(NativeLiveUnavailableReason::TransportUnavailable)
            }
            200 => InventoryReadFailure::Invalid { code: None },
            _ => InventoryReadFailure::Invalid {
                code: Some(i64::from(status)),
            },
        };
        assert_eq!(failure, expected, "HTTP status {status}");
    }
}

#[tokio::test]
async fn configured_http_only_published_discovery_unavailability_is_recoverable() {
    use crate::picker_runtime_status::NativeLiveUnavailableReason;
    let published = json!({"kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL});
    let adapter = json!({"mcpResult":"error","kind":"rejected","serviceKind":"unavailable","stage":"discovery","effect":"none","message":PRIVATE_SENTINEL,"code":-32050,"data":published});
    let tool = |id: i64, payload: Value| json!({"jsonrpc":"2.0","id":id,"result":{"content":[],"isError":true,"structuredContent":payload}});
    let rpc = |id: i64, code: i64, data: Value| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":PRIVATE_SENTINEL,"data":data}});
    let mut wrong_stage = adapter.clone();
    wrong_stage["stage"] = json!("response");
    let mut unsupported = adapter.clone();
    unsupported["data"]["kind"] = json!("unsupportedCapability");
    let invalid_native = json!({"kind":"invalidNativeInventory","stage":"discovery","source":"backgroundJobs","reason":"invalidOutput","message":PRIVATE_SENTINEL});
    let native_failure = InventoryReadFailure::InvalidNative {
        inventory_source: collaboration_client::protocol::NativeInventorySource::BackgroundJobs,
        reason: collaboration_client::protocol::NativeInventoryInvalidReason::InvalidOutput,
    };
    let cases = [
        (rpc(1, -32602, invalid_native.clone()), native_failure),
        (
            tool(
                1,
                json!({"mcpResult":"error","kind":"rejected","serviceKind":"unavailable","stage":"discovery","effect":"none","message":PRIVATE_SENTINEL,"code":-32602,"data":invalid_native}),
            ),
            native_failure,
        ),
        (
            tool(
                1,
                json!({"mcpResult":"error","kind":"invalidNativeInventory","stage":"discovery","source":"backgroundJobs","reason":"invalidOutput","message":PRIVATE_SENTINEL}),
            ),
            native_failure,
        ),
        (
            tool(1, adapter.clone()),
            InventoryReadFailure::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (
            rpc(1, -32050, published.clone()),
            InventoryReadFailure::Unavailable(NativeLiveUnavailableReason::ProviderUnavailable),
        ),
        (
            rpc(1, -32602, published.clone()),
            InventoryReadFailure::Invalid { code: Some(-32602) },
        ),
        (
            rpc(1, -32051, published.clone()),
            InventoryReadFailure::Invalid { code: Some(-32051) },
        ),
        (
            rpc(2, -32050, published),
            InventoryReadFailure::Invalid { code: None },
        ),
        (
            tool(2, adapter),
            InventoryReadFailure::Invalid { code: None },
        ),
        (
            tool(1, wrong_stage),
            InventoryReadFailure::Invalid { code: Some(-32050) },
        ),
        (
            tool(1, unsupported),
            InventoryReadFailure::Invalid { code: Some(-32050) },
        ),
        (
            tool(
                1,
                json!({"mcpResult":"error","kind":"futureUnavailable","stage":"discovery","message":PRIVATE_SENTINEL}),
            ),
            InventoryReadFailure::Invalid { code: Some(-32050) },
        ),
        (
            tool(
                1,
                json!({"mcpResult":"error","kind":"unavailable","stage":"discovery","message":PRIVATE_SENTINEL,"data":{"private":PRIVATE_SENTINEL}}),
            ),
            InventoryReadFailure::Invalid { code: None },
        ),
    ];
    for (reply, expected) in cases {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let router = axum::Router::new().route(
                "/mcp",
                axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
                    let reply = reply.clone();
                    async move {
                        assert_eq!(request["params"]["name"], "provider_sessions_list");
                        axum::Json(reply)
                    }
                }),
            );
            axum::serve(listener, router).await.unwrap();
        });
        let profile = profile_for_address(&format!("http://{address}/mcp"));
        let request = ProviderSessionListParams {
            endpoint: serde_json::from_value(
                json!({"serviceId":profile.service_id,"endpointId":"claude-source"}),
            )
            .unwrap(),
            view: collaboration_client::protocol::NativeSessionView::Loaded,
            scope: collaboration_client::protocol::NativeSessionScope::Any,
            source: collaboration_client::protocol::NativeSessionSource::All,
            query: None,
            page_size: 100,
            cursor: None,
        };
        let failure = HttpConfiguredInventoryClient::new(&profile)
            .unwrap()
            .provider_page(&request)
            .await
            .unwrap_err();
        assert_eq!(failure, expected);
        assert!(!format!("{failure:?}").contains(PRIVATE_SENTINEL));
        assert!(!failure.into_client().to_string().contains(PRIVATE_SENTINEL));
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn configured_http_provider_error_keeps_actual_code_without_private_data() {
    for code in [-32602_i64, -32051_i64] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let router=axum::Router::new().route("/mcp",axum::routing::post(move |axum::Json(request):axum::Json<Value>|async move {
                assert_eq!(request["params"]["name"],"provider_sessions_list");
                axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":code,"message":PRIVATE_SENTINEL,"data":{"private":PRIVATE_SENTINEL}}}))
            }));
            axum::serve(listener, router).await.unwrap();
        });
        let profile = profile_for_address(&format!("http://{address}/mcp"));
        let error=HttpConfiguredInventoryClient::new(&profile).unwrap()
            .call::<Value>("provider_sessions_list",&json!({"endpoint":{"serviceId":profile.service_id,"endpointId":"claude-source"},"view":"loaded","scope":{"kind":"any"},"source":"all","query":null,"pageSize":100,"cursor":null})).await.unwrap_err();
        let ClientError::Rejected { code: actual, .. } = &error else {
            panic!("retain actual rejection code: {error:?}")
        };
        assert_eq!(
            *actual, code,
            "unknown/invalid rejection cannot masquerade as published availability"
        );
        assert!(
            !format!("{error:?}").contains(PRIVATE_SENTINEL),
            "error text/data must remain private"
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
