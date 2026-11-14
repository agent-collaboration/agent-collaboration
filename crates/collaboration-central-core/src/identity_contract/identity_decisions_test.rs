use super::*;
use serde_json::json;

fn request() -> serde_json::Value {
    json!({
        "organizationId":"018f1111-2222-7000-8000-000000000001",
        "approver":"018f1111-2222-7000-8000-000000000002",
        "permissionChecked":"machine:enroll",
        "requestId":"018f1111-2222-7000-8000-000000000003",
        "installation":"018f1111-2222-7000-8000-000000000004",
        "peer":"018f1111-2222-7000-8000-000000000004",
        "label":"machine",
        "allowedWorkspaces":[],
        "approverSecondFactorAt":null,
        "codeExpiresAt":1800000600
    })
}

#[test]
fn device_login_requires_checked_permission_true_deadline_uuid_v7_and_bounded_workspaces() {
    assert!(serde_json::from_value::<DecideDeviceLogin>(request()).is_ok());
    for field in ["codeExpiresAt", "approverSecondFactorAt"] {
        for invalid in [json!(-1), json!(i64::MIN), json!(1.5), json!("1800000600")] {
            let mut value = request();
            value[field] = invalid;
            assert!(serde_json::from_value::<DecideDeviceLogin>(value).is_err());
        }
        let mut epoch = request();
        epoch[field] = json!(0);
        let parsed: DecideDeviceLogin = serde_json::from_value(epoch.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), epoch);
    }
    let mut null_deadline = request();
    null_deadline["codeExpiresAt"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<DecideDeviceLogin>(null_deadline).is_err());
    for field in [
        "permissionChecked",
        "approverSecondFactorAt",
        "codeExpiresAt",
    ] {
        let mut missing = request();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<DecideDeviceLogin>(missing).is_err());
    }
    for (field, value) in [
        ("permissionChecked", json!("machine:revoke")),
        (
            "installation",
            json!("018f1111-2222-4000-8000-000000000004"),
        ),
        (
            "formerServiceId",
            json!("018f1111-2222-7000-8000-000000000005"),
        ),
    ] {
        let mut invalid = request();
        invalid[field] = value;
        assert!(serde_json::from_value::<DecideDeviceLogin>(invalid).is_err());
    }
    let mut bounded = request();
    let workspace = json!("018f1111-2222-7000-8000-000000000005");
    bounded["allowedWorkspaces"] = json!(vec![workspace.clone(); 100]);
    assert!(serde_json::from_value::<DecideDeviceLogin>(bounded.clone()).is_ok());
    bounded["allowedWorkspaces"] = json!(vec![workspace; 101]);
    assert!(serde_json::from_value::<DecideDeviceLogin>(bounded).is_err());
}

#[test]
fn approved_reply_keeps_all_three_snapshot_fields_and_refusal_window_is_nonzero() {
    let request: DecideDeviceLogin = serde_json::from_value(request()).unwrap();
    let approved = DeviceLoginDecision::Approved {
        enrollment_id: "018f1111-2222-7000-8000-000000000006"
            .to_owned()
            .try_into()
            .unwrap(),
        label: request.label,
        allowed_workspaces: request.allowed_workspaces,
    };
    let wire = serde_json::to_value(approved).unwrap();
    assert_eq!(
        wire,
        json!({
            "result":"approved","enrollmentId":"018f1111-2222-7000-8000-000000000006",
            "label":"machine","allowedWorkspaces":[]
        })
    );
    assert!(
        serde_json::from_value::<DeviceLoginDecision>(json!({
            "result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":0}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<DeviceLoginDecision>(json!({
            "result":"refused","reason":{"kind":"notOwner"}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<DeviceLoginDecision>(json!({
            "result":"refused","reason":{"kind":"notPermitted"}
        }))
        .is_ok()
    );
}

#[test]
fn machine_grant_serializes_superseded_only_when_an_old_client_was_ended() {
    let enrollment_id = "018f1111-2222-7000-8000-000000000006"
        .to_owned()
        .try_into()
        .unwrap();
    let installation = "018f1111-2222-7000-8000-000000000004"
        .to_owned()
        .try_into()
        .unwrap();
    let mut grant = MachineTokenDecision::Grant {
        installation,
        enrollment_id,
        decided_at: UnixSeconds(1800000000),
        grant_expires_at: UnixSeconds(1800000600),
        superseded: None,
    };
    for field in ["decidedAt", "grantExpiresAt"] {
        for invalid in [
            json!(-1),
            json!(i64::MIN),
            json!(1.5),
            json!("1800000600"),
            serde_json::Value::Null,
        ] {
            let mut value = serde_json::to_value(&grant).unwrap();
            value[field] = invalid;
            assert!(serde_json::from_value::<MachineTokenDecision>(value).is_err());
        }
        let mut epoch = serde_json::to_value(&grant).unwrap();
        epoch[field] = json!(0);
        let parsed: MachineTokenDecision = serde_json::from_value(epoch.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), epoch);
    }
    assert!(
        serde_json::to_value(&grant)
            .unwrap()
            .get("superseded")
            .is_none()
    );
    let MachineTokenDecision::Grant { superseded, .. } = &mut grant else {
        unreachable!()
    };
    *superseded = Some(EndedMachineClient {
        enrollment_id: "018f1111-2222-7000-8000-000000000007"
            .to_owned()
            .try_into()
            .unwrap(),
        client_id: "018f1111-2222-7000-8000-000000000007"
            .to_owned()
            .try_into()
            .unwrap(),
    });
    assert_eq!(
        serde_json::to_value(grant).unwrap()["superseded"],
        json!({
            "enrollmentId":"018f1111-2222-7000-8000-000000000007",
            "clientId":"018f1111-2222-7000-8000-000000000007"
        })
    );
}
