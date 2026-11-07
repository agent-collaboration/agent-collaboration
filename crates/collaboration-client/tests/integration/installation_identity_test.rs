//! Schema-valid responses still have to describe this installation's peer.
use super::scripted_api::{SCRIPTED_SERVICE_ID, ScriptedApi};
use collaboration_client::{ClientError, CollaborationClient};
use collaboration_protocol::InstallationIdentity;
use serde_json::json;

#[tokio::test]
async fn installation_link_requires_the_identity_peer_and_a_nonempty_endpoint() {
    let peer = uuid::Uuid::now_v7().to_string();
    let other_peer = uuid::Uuid::now_v7().to_string();
    for (link_peer, endpoint, valid) in [
        (peer.as_str(), "wss://central.example.test/session", true),
        (
            other_peer.as_str(),
            "wss://central.example.test/session",
            false,
        ),
        (peer.as_str(), "", false),
    ] {
        // Arrange: every response passes typed decoding before the semantic check.
        let identity = json!({
            "serviceId": SCRIPTED_SERVICE_ID,
            "machineLabel": "fixture-host",
            "peerId": peer,
            "central": {
                "peer": link_peer,
                "endpoint": endpoint,
                "state": { "kind": "disconnected", "since": "2026-11-06T12:00:00Z" },
                "lastDrop": null,
                "lastFailure": null
            }
        });
        let typed: InstallationIdentity = serde_json::from_value(identity.clone()).unwrap();
        let (api, mut calls) = ScriptedApi::start().unwrap();
        let client = CollaborationClient::connect(api.directory(), "link-validation", "1")
            .await
            .unwrap();

        // Act: cross the real service socket and response decoder.
        let answer = async {
            let call = calls.next().await.unwrap();
            assert_eq!(call.tool, "installation_show");
            call.succeed(identity);
        };
        let (result, ()) = tokio::join!(client.installation_show(), answer);

        // Assert: identity equality and an actual endpoint are required together.
        if valid {
            assert_eq!(result.unwrap(), typed);
        } else {
            assert!(
                matches!(
                    result,
                    Err(ClientError::Protocol("invalid central link record"))
                ),
                "schema-valid unrelated or empty-endpoint link was accepted: {result:?}"
            );
        }
    }
}
