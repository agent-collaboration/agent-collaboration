//! Public dispatch evidence, separate from the decoder's isolated DAG check.
use super::*;
use serde_json::json;

#[tokio::test]
async fn registry_public_call_refuses_copy_authority_and_allows_session_peer_read() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let mut envelope = json!({
        "source": {"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"},
        "sourceFace": "copy",
        "authority": {"kind": "object"},
        "destination": {"kind": "registry"},
        "destinationFace": "registry",
        "identity": {"operation": "019f0000-0000-7000-8000-000000000806", "step": "object"},
        "kind": "central.installation-peer",
        "target": {"installation": "550e8400-e29b-41d4-a716-446655440806"},
        "request": null,
    });
    let result = async {
        assert!(matches!(
            fixture
                .runtime
                .on_call(serde_json::to_vec(&envelope)?)
                .await,
            Err(CallFailure::NotSent {
                reason: NotSentReason::Unreachable { last_seen: None }
            })
        ));

        // This otherwise identical permitted source reaches the real Registry
        // read. Its absence is a serialized Option<PeerId>, not a refusal.
        envelope["source"] = json!({
            "kind": "connection", "value": "550e8400-e29b-41d4-a716-446655440806"
        });
        envelope["sourceFace"] = json!("session");
        let reply = fixture
            .runtime
            .on_call(serde_json::to_vec(&envelope)?)
            .await
            .map_err(|failure| format!("permitted Session peer read: {failure:?}"))?;
        assert_eq!(serde_json::from_slice::<Option<PeerId>>(&reply)?, None);
        println!(
            "CALL_EDGE_CASE public_Copy_Registry_NotSent_and_Session_Registry_peer_read passed"
        );
        Ok(())
    }
    .await;
    fixture.objects.close().await?;
    result
}
