use message_board::*;
use serde_json::{Value, json};

mod support;

fn actor_value(actor: Value) -> Value {
    json!({"actor":actor,"installation":"00000000-0000-4000-8000-000000000001",
        "machine":"test machine"})
}

#[test]
fn recorded_actors_distinguish_declared_identity_from_router_component() {
    for actor in [
        json!({"kind":"declared","identity":{"kind":"human","humanId":"writer"}}),
        json!({"kind":"router","component":"subscriptionDelivery"}),
    ] {
        let value = actor_value(actor);
        let decoded = serde_json::from_value::<RecordedActor>(value.clone());
        assert!(decoded.is_ok(), "recording actor: {decoded:?}");
        assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), value);
    }
    for actor in [
        json!({"kind":"router","component":"unknown"}),
        json!({"kind":"router","component":"subscriptionDelivery","identity":{"kind":"human","humanId":"writer"}}),
        json!({"kind":"declared","identity":{"kind":"router","component":"subscriptionDelivery"}}),
    ] {
        assert!(serde_json::from_value::<RecordedActor>(actor_value(actor)).is_err());
    }
}

#[test]
fn a_router_recording_actor_cannot_become_an_authority_holder() {
    let project = ProjectId::generate();
    let mut envelope = serde_json::to_value(support::record_envelope(
        RecordId::generate(),
        project.clone(),
    ))
    .unwrap();
    let declared =
        actor_value(json!({"kind":"declared","identity":{"kind":"human","humanId":"writer"}}));
    envelope["actor"] = declared.clone();
    let mut assignment = json!({"envelope":envelope,"assignmentId":RoleAssignmentId::generate(),
        "subject":{"kind":"project","subject":project},"role":"lead","holder":declared,
        "grantedBy":declared,"model":null,"lineage":null,"ended":null,"ownerGrant":null});
    assert!(serde_json::from_value::<RoleAssignment>(assignment.clone()).is_ok());
    assignment["holder"] = actor_value(json!({"kind":"router","component":"subscriptionDelivery"}));
    assert!(serde_json::from_value::<RoleAssignment>(assignment).is_err());
}

#[test]
fn public_identity_inputs_reject_router_actors() {
    let router = json!({"kind":"router","component":"subscriptionDelivery"});
    assert!(serde_json::from_value::<Identity>(router.clone()).is_err());
    let request = json!({"requestId":RequestId::generate(),"projectId":ProjectId::generate(),
        "actor":router,"actingFor":null,"observedEpoch":null,"resubmits":null,
        "protocolVersion":1,"operation":{"kind":"intentClaim","intentId":NotifyIntentId::generate()}});
    assert!(serde_json::from_value::<ProjectRequest>(request).is_err());
}

#[test]
fn public_project_requests_do_not_expose_the_automated_claim_entry() {
    let request = json!({"requestId":RequestId::generate(),"projectId":ProjectId::generate(),
        "actor":{"kind":"human","humanId":"writer"},"actingFor":null,"observedEpoch":null,
        "resubmits":null,"protocolVersion":1,
        "operation":{"kind":"intentClaim","intentId":NotifyIntentId::generate()}});
    assert!(serde_json::from_value::<ProjectRequest>(request).is_err());
}
