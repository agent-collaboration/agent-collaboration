use collaboration_protocol::ThreadSubscribeRequest;
use message_board::*;

#[test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the contract; Result propagates fixture errors.
fn subscription_writes_require_request_identity_and_protocol_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let value = serde_json::json!({
        "requestId":RequestId::generate(),"protocolVersion":ProtocolVersion::CURRENT,
        "observedEpoch":1,"resubmits":null,"actingFor":null,
        "actor":{"kind":"human","humanId":"reader"},
        "scope":{"kind":"project","projectId":ProjectId::generate()},
        "policy":{"mode":null,"whenIdle":null,"quietSeconds":null,"capSeconds":null,"lifetimeSeconds":null}
    });
    let _: ThreadSubscribeRequest = serde_json::from_value(value.clone())?;
    let mut missing = value;
    missing
        .as_object_mut()
        .ok_or("fixture object")?
        .remove("requestId");
    assert!(serde_json::from_value::<ThreadSubscribeRequest>(missing).is_err());
    Ok(())
}

#[test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the contract; Result propagates fixture errors.
fn wait_renewals_have_independent_declared_request_keys() -> Result<(), Box<dyn std::error::Error>>
{
    let value = serde_json::json!({"actor":{"kind":"human","humanId":"reader"},"actingFor":null,"protocolVersion":1,"filter":{"kind":"all"},"maxWaitSeconds":0,"renewals":[{"requestId":RequestId::generate(),"resubmits":null,"observedEpoch":1,"scope":{"kind":"project","projectId":ProjectId::generate()}}]});
    let _: collaboration_protocol::ThreadSubscriptionWaitRequest =
        serde_json::from_value(value.clone())?;
    let mut duplicate = value.clone();
    let entries = duplicate["renewals"].as_array_mut().ok_or("renewals")?;
    entries.push(entries[0].clone());
    assert!(
        serde_json::from_value::<collaboration_protocol::ThreadSubscriptionWaitRequest>(duplicate)
            .is_err()
    );
    let mut anonymous = value;
    anonymous["renewals"][0]
        .as_object_mut()
        .ok_or("entry")?
        .remove("requestId");
    assert!(
        serde_json::from_value::<collaboration_protocol::ThreadSubscriptionWaitRequest>(anonymous)
            .is_err()
    );
    Ok(())
}
