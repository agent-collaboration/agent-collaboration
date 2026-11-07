use super::*;
use crate::{ConversationCancelInput, ConversationPromptInput};
use serde_json::json;

#[test]
fn actor_producer_and_request_consumers_preserve_the_established_wire_shapes() {
    let session = json!({"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001",
        "endpointId":"codex-local"},"sessionId":"fixture-requester"});
    let target = json!({"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001",
        "endpointId":"claude-local"},"sessionId":"fixture-target"});
    for wire in [
        session.clone(),
        json!({"kind":"session","session":session}),
        json!({"kind":"human","humanId":"fixture-operator"}),
    ] {
        // Arrange: the real actor serializer produces the bytes consumed by both DTOs.
        let actor: ConversationActor = serde_json::from_value(wire.clone()).expect("actor");
        let encoded = serde_json::to_value(&actor).expect("serialize actor");
        assert_eq!(encoded, wire);
        let expected = if wire["kind"] == "human" {
            json!({"humanId":"fixture-operator"})
        } else {
            session.clone()
        };
        // Act
        let prompt: ConversationPromptInput = serde_json::from_value(json!({
            "target":target,"requestedBy":encoded,"message":{"kind":"humanUser","text":"allowed separate authorship"}
        })).expect("prompt consumer");
        let cancel: ConversationCancelInput = serde_json::from_value(json!({
            "operationId":collaboration_protocol::OperationId::generate(),
            "targetOperationId":collaboration_protocol::OperationId::generate(),
            "target":target,"requestedBy":encoded
        }))
        .expect("cancel consumer");
        // Assert: both consumers use the same authoritative conversion.
        for requester in [prompt.requested_by, cancel.requested_by] {
            assert_eq!(
                serde_json::to_value(requester.provider_identity().expect("identity"))
                    .expect("wire identity"),
                expected
            );
        }
    }
}
