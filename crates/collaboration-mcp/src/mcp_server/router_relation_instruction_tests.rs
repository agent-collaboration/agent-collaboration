use crate::RouterExecutableCheck;
use crate::api_test_harness::{ServedApi, TEST_PROTOCOL_VERSION, api_config, test_identity};
use collaboration_protocol::RouterExecutableRelation;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The Host's check as a client sees it: what it reports changes only when it is asked.
struct ReplaceableExecutable(Mutex<RouterExecutableRelation>);

impl RouterExecutableCheck for ReplaceableExecutable {
    fn observe(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RouterExecutableRelation> + Send + '_>>
    {
        let relation = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Box::pin(std::future::ready(relation))
    }
}

async fn initialize(api: &ServedApi) -> Value {
    let (status, response) = api
        .post(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": TEST_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "stale-banner-proof", "version": "1"}
            }
        }))
        .await;
    assert!(
        status.is_success(),
        "initialize answered {status}: {response}"
    );
    response["result"].clone()
}

#[tokio::test]
async fn each_initialize_asks_the_check_so_a_replaced_executable_is_reported_stale() {
    let directory = tempfile::tempdir().expect("service directory");
    let check = Arc::new(ReplaceableExecutable(Mutex::new(
        RouterExecutableRelation::Match,
    )));
    let mut config = api_config(test_identity().build(), directory.path());
    config.router_executable_check = check.clone();
    let api = ServedApi::tcp(&config).await;
    let before = initialize(&api).await;

    *check.0.lock().expect("relation") = RouterExecutableRelation::Drift {
        running_version: "0.1.36".to_owned(),
        installed_version: Some("0.1.37".to_owned()),
    };
    let after = initialize(&api).await;

    assert!(before["instructions"].is_null(), "{before}");
    assert_eq!(
        after["instructions"].as_str(),
        Some(
            "⚠ Router Host is stale (running 0.1.36, installed 0.1.37); run `codex-router host restart`"
        )
    );
    api.stop().await;
}
