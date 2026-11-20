//! Loaded native delivery and reconciliation without a route store.
use super::*;
use agent_automation::RouteEffectEvidence;
use std::sync::Mutex;

#[path = "native_loaded_permission_fixture.rs"]
mod policy_fixture;
use policy_fixture::*;

#[path = "native_policy_preservation_tests.rs"]
mod preservation_policy_tests;
#[path = "native_policy_queue_tests.rs"]
mod queue_policy_tests;

#[tokio::test]
async fn a_message_without_a_client_id_gets_a_uuidv7_correlation() {
    let fixture = NativeDeliveryFixture::create().unwrap();
    let server = policy_server(
        &fixture,
        LoadedDeliveryScenario {
            first_unloaded: false,
            mode: MessageDelivery::Auto,
        },
    )
    .unwrap();
    let mut minted = Vec::new();
    for _ in 0..2 {
        let mut request = fixture.message_request();
        request.params.client_user_message_id = None;
        let outcome = tokio::time::timeout(Duration::from_secs(5), dispatch_message(request))
            .await
            .unwrap();
        let NativeMessageOutcome::Accepted(receipt) = outcome else {
            panic!("a matching policy accepts the message");
        };
        minted.push(String::from(receipt.client_user_message_id));
    }
    let exchanges = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    for (correlation, requests) in minted.iter().zip(&exchanges) {
        assert_eq!(
            uuid::Uuid::parse_str(correlation)
                .unwrap()
                .get_version_num(),
            7,
            "minted correlation {correlation}"
        );
        assert!(
            requests
                .iter()
                .any(|request| request["method"] == "turn/start"
                    && request["params"]["clientUserMessageId"] == correlation.as_str()),
            "the native input carries the minted correlation"
        );
    }
}
