//! Rust-to-Rust C7 shapes from PD373b4704, without a second K publication.
#![cfg(test)]
use collaboration_central_core::identity_contract::{
    ApprovalActivation, ApprovalAuthorization, ApprovalCancellation, AuthorizeDeviceApproval,
    DecideDeviceLogin, SettleDeviceApproval,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::fmt::Debug;

fn decision_literal() -> Value {
    json!({
        "organizationId":"019a3dce-2000-7000-8000-000000000001",
        "approver":"019a3dce-2000-7000-8000-000000000002",
        "requestId":"019a3dce-2000-7000-8000-000000000003",
        "installation":"019a3dce-2000-7000-8000-000000000004",
        "peer":"019a3dce-2000-7000-8000-000000000005",
        "label":"fixture machine",
        "allowedWorkspaces":["019a3dce-2000-7000-8000-000000000006"],
        "approverSecondFactorAt":1793448000_i64
    })
}
fn assert_roundtrip<T: Serialize + DeserializeOwned + Eq + Debug>(wire: Value) {
    let decoded: T = serde_json::from_value(wire.clone()).unwrap();
    let encoded = serde_json::to_value(&decoded).unwrap();
    assert_eq!(encoded, wire);
    assert_eq!(
        serde_json::from_slice::<T>(&serde_json::to_vec(&decoded).unwrap()).unwrap(),
        decoded
    );
    let mut extra = wire;
    extra
        .as_object_mut()
        .unwrap()
        .insert("notInTheContract".into(), json!(true));
    assert!(serde_json::from_value::<T>(extra).is_err());
}

// Catches a renamed, flattened or partially rebuilt decision. The real shared
// DecideDeviceLogin serializer is compared with the literal PD wrapper.
#[test]
fn approval_authorization_forwards_the_shared_decision_unchanged() {
    let decision: DecideDeviceLogin = serde_json::from_value(decision_literal()).unwrap();
    let request = AuthorizeDeviceApproval {
        decision: decision.clone(),
    };
    let expected = json!({"decision":decision_literal()});
    assert_eq!(serde_json::to_value(&request).unwrap(), expected);
    let decoded: AuthorizeDeviceApproval = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(decoded.decision, decision);
    assert_roundtrip::<AuthorizeDeviceApproval>(expected);
    let mut without_second_factor = decision_literal();
    without_second_factor
        .as_object_mut()
        .unwrap()
        .remove("approverSecondFactorAt");
    assert_roundtrip::<AuthorizeDeviceApproval>(json!({"decision":without_second_factor}));
    assert!(serde_json::from_value::<AuthorizeDeviceApproval>(decision_literal()).is_err());
}

#[test]
fn activate_and_cancel_share_only_the_organization_and_request_id() {
    let request = json!({
        "organizationId":"019a3dce-2000-7000-8000-000000000001",
        "requestId":"019a3dce-2000-7000-8000-000000000003"
    });
    assert_roundtrip::<SettleDeviceApproval>(request.clone());
    let mut missing = request.clone();
    missing.as_object_mut().unwrap().remove("requestId");
    assert!(serde_json::from_value::<SettleDeviceApproval>(missing).is_err());
    let mut invalid = request;
    invalid["requestId"] = json!("not-a-uuid7");
    assert!(serde_json::from_value::<SettleDeviceApproval>(invalid).is_err());
}

// These exact result/field spellings are the PD oracle; a receiver must never
// decode an unknown result as a successful reservation or settlement.
#[test]
fn directory_approval_answers_roundtrip_all_named_results() {
    assert_roundtrip::<ApprovalAuthorization>(json!({"result":"reserved"}));
    for reason in [
        json!({"kind":"notOwner"}),
        json!({"kind":"machineInAnotherOrganization"}),
        json!({"kind":"unknownWorkspace"}),
        json!({"kind":"secondFactorRequired","withinSeconds":900}),
    ] {
        assert_roundtrip::<ApprovalAuthorization>(json!({"result":"refused","reason":reason}));
    }
    assert_roundtrip::<ApprovalActivation>(
        json!({"result":"activated","enrollmentId":"019a3dce-2000-7000-8000-000000000007"}),
    );
    assert_roundtrip::<ApprovalActivation>(json!({"result":"notReserved"}));
    assert_roundtrip::<ApprovalCancellation>(json!({"result":"cancelled"}));
    assert_roundtrip::<ApprovalCancellation>(json!({"result":"notReserved"}));
}

#[test]
fn directory_approval_answers_refuse_unknown_tags_and_other_steps() {
    for wire in [
        json!({"result":"unknown"}),
        json!({"result":"activated","enrollmentId":"019a3dce-2000-7000-8000-000000000007"}),
        json!({"result":"refused","reason":{"kind":"unknown"}}),
    ] {
        assert!(serde_json::from_value::<ApprovalAuthorization>(wire).is_err());
    }
    for wire in [
        json!({"result":"unknown"}),
        json!({"result":"cancelled"}),
        json!({"result":"activated","enrollment_id":"019a3dce-2000-7000-8000-000000000007"}),
    ] {
        assert!(serde_json::from_value::<ApprovalActivation>(wire).is_err());
    }
    for wire in [
        json!({"result":"unknown"}),
        json!({"result":"reserved"}),
        json!({"state":"cancelled"}),
    ] {
        assert!(serde_json::from_value::<ApprovalCancellation>(wire).is_err());
    }
}
