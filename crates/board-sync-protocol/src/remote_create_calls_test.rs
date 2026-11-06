use super::*;
use crate::{BoardConsumerRequest, BoardPayload, FleetRequestEnvelope};
use serde_json::json;

#[test]
fn remote_create_calls_keep_the_existing_non_mail_fleet_envelope_and_ids() {
    let id = "019f0000-0000-7000-8000-000000000001";
    let request = json!({"requestId":id,"requester":{"endpoint":{"serviceId":id,"endpointId":"claude-local"},"sessionId":"requester"},
        "targetMachine":id,"command":{"endpoint":"codex-local","workingDirectory":"/path/on/B","access":"workspace-write",
            "model":null,"effort":null,"mode":null,"firstInput":null}});
    for payload in [
        json!({"kind":"remoteCreate","value":request}),
        json!({"kind":"remoteCreateStatus","value":{"requestId":id}}),
    ] {
        let call: RemoteCreateCall = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(call.request_id().as_str(), id);
        let envelope = BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: call.request_id().clone(),
            operation: BoardPayload::encode(&call).unwrap(),
        });
        let bytes = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            bytes,
            json!({"kind":"fleet","value":{"requestId":id,"operation":payload}})
        );
        let BoardConsumerRequest::Fleet(decoded) = serde_json::from_value(bytes).unwrap() else {
            panic!("wrong call family");
        };
        assert_eq!(
            decoded.operation.decode::<RemoteCreateCall>().unwrap(),
            call
        );
        let mut extra = payload;
        extra["extra"] = json!(true);
        assert!(serde_json::from_value::<RemoteCreateCall>(extra).is_err());
    }
    assert!(
        serde_json::from_value::<RemoteCreateCall>(json!({"kind":"futureCreate","value":{}}))
            .is_err()
    );
}
