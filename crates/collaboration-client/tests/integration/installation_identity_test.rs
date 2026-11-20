//! The installation's peer is decoded through the actual service socket.
use super::scripted_api::{SCRIPTED_SERVICE_ID, ScriptedApi};
use collaboration_client::CollaborationClient;
use serde_json::json;

#[tokio::test]
async fn installation_show_decodes_the_authoritative_peer() {
    let peer = uuid::Uuid::now_v7().to_string();
    let identity =
        json!({"serviceId":SCRIPTED_SERVICE_ID,"machineLabel":"fixture-host","peerId":peer});
    let (api, mut calls) = ScriptedApi::start().unwrap();
    let client = CollaborationClient::connect(api.directory(), "installation-peer", "1")
        .await
        .unwrap();
    let answer = async {
        let call = calls.next().await.unwrap();
        assert_eq!(call.tool, "installation_show");
        call.succeed(identity);
    };
    let (result, ()) = tokio::join!(client.installation_show(), answer);
    assert_eq!(result.unwrap().peer_id.as_str(), peer);
}
