//! Actual HTTP JSON-RPC/tool envelopes, before any loader classification.
use super::*;
use serde_json::{Value, json};

const PRIVATE_SENTINEL: &str = "PRIVATE_INVENTORY_FAILURE_SENTINEL_DO_NOT_PRESENT";

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
    let cases = [
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
