//! The actual fixed source executable classifies API failures before SSH wire output.
//! Only the external API response is scripted; source parsing/client/decoder stay real.
use serde_json::json;
use std::time::Duration;

use agent_session_inventory::test_support::fake_api_support;
use fake_api_support::{FakeCollaborationApi, FakeReply};

#[tokio::test]
async fn fixed_source_provider_read_preserves_availability_vs_invalid_without_private_data() {
    let service = "00000000-0000-4000-8000-000000000001";
    let epoch = "00000000-0000-4000-8000-000000000002";
    let sentinel = "SOURCE_READ_PRIVATE_SENTINEL";
    for (code, data, expected_reason) in [
        (
            -32050,
            json!({"kind":"unavailable","stage":"discovery","message":sentinel}),
            "sourceUnavailable",
        ),
        (
            -32602,
            json!({"kind":"unavailable","stage":"discovery","message":sentinel}),
            "invalidRequest",
        ),
        (
            -32051,
            json!({"kind":"unavailable","stage":"discovery","message":sentinel}),
            "invalidRequest",
        ),
        (
            -32050,
            json!({"kind":"futureFailure","stage":"discovery","message":sentinel}),
            "invalidRequest",
        ),
        (
            -32050,
            json!({"kind":"unavailable","stage":"discovery","message":sentinel,"private":sentinel}),
            "invalidRequest",
        ),
    ] {
        let mut api = FakeCollaborationApi::new(service, epoch).unwrap();
        let directory = api.directory().to_owned();
        let serving = api.serve_then_watch(
            vec![FakeReply::Error(
                json!({"code":code,"message":sentinel,"data":data}),
            )],
            Duration::from_millis(25),
        );
        let peer = tokio::spawn(serving);
        let request = json!({"serviceDirectory":directory,"expectedService":service,"operation":{"kind":"providerSessions","params":{
            "endpoint":{"serviceId":service,"endpointId":"claude-local"},"view":"loaded","scope":{"kind":"any"},"source":"interactive","query":null,"pageSize":100,"cursor":null
        }}});
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-sessions"))
                .args(["--source-request-v1", &request.to_string()])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        let observed = tokio::time::timeout(Duration::from_secs(5), peer)
            .await
            .expect("fixed source must reach scripted API")
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(observed.calls.len(), 1, "one read without retry");
        assert!(observed.further_call.is_none(), "no recovery or mutation");
        assert_eq!(
            observed
                .calls
                .first()
                .unwrap()
                .get("tool")
                .and_then(serde_json::Value::as_str),
            Some("provider_sessions_list")
        );
        assert_eq!(output.status.code(), Some(2));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            response,
            json!({"kind":"rejected","reason":expected_reason})
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains(sentinel));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(sentinel));
    }
}
