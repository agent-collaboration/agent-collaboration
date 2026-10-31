use serde_json::{Value, json};

pub fn explicit_negatives(name: &str, literals: &[Value]) -> Vec<Value> {
    let mut invalid = vec![Value::Null];
    match name {
        "DecideDeviceLogin" | "PresentedMachine" | "AdmittedMachine" => {
            if let Some(first) = literals.first() {
                for peer in ["00000000-0000-4000-8000-000000000001", "019A3DCE-2000-7000-8000-000000000001"] {
                    let mut value = first.clone(); value["peer"] = json!(peer); invalid.push(value);
                }
            }
        }
        "OwnerRefusal" => invalid.push(json!({"kind":"secondFactorRequired","withinSeconds":900})),
        "IdentityLookupResult" | "WorkspaceLookupResult" | "MachineLookupResult" | "CommandStatusResult" | "WhoIsLoggedInResult" => invalid.push(json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":900}})),
        "IdentityLookup" => invalid.push(json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","subject":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000001"}})),
        "AbsolutePath" => invalid.extend([
            json!("relative"), json!("C:\\x"), json!(""), json!("/nul\0"),
            json!(format!("/{}", "a".repeat(4096))),
            json!(format!("/{}", "é".repeat(2048))),
            json!(format!("/{}aaaa", "😀".repeat(1023))),
        ]),
        "RepositoryLocator" => invalid.extend([json!(""), json!("r".repeat(2049)), json!(format!("{}a", "é".repeat(1024)))]),
        "ModelName" => invalid.extend([json!(""), json!("m".repeat(257)), json!(format!("{}a", "é".repeat(128))), json!("é".repeat(256))]),
        "MachineLabel" => invalid.push(json!(format!("{}a", "é".repeat(128)))),
        "SessionRef" => {
            if let Some(first) = literals.first() {
                let mut value = first.clone(); value["sessionId"] = json!(format!("{}a", "é".repeat(2048))); invalid.push(value);
            }
        }
        "CommitId" => {
            invalid.extend([39,41,63,65].map(|length| json!("a".repeat(length))));
            invalid.extend([json!("A".repeat(40)), json!("z".repeat(64))]);
        }
        "OrganizationId" | "EnrollmentId" | "RemoteStartId" | "IsolationInstanceId" | "PeerId" |
        "PersonId" | "ClientId" | "DeviceLoginRequestId" | "AdmissionId" | "CommandId" => invalid.extend([
            json!("00000000-0000-4000-8000-000000000001"),
            json!("019A3DCE-2000-7000-8000-000000000001"),
            json!("019a3dce-2000-7000-0000-000000000001"),
        ]),
        "SequenceNumber" => invalid.extend([json!(9007199254740992_u64), json!(-1), json!(1.5)]),
        "TokenLifetimeSeconds" => invalid.extend([json!(0),json!(59),json!(3601)]),
        "IsolationKind" => invalid.extend([json!("Host"),json!("process-sandbox"),json!("unknown")]),
        "MachineEnvironment" => invalid.extend([json!("Physical"),json!("cloud-vm"),json!("unknown")]),
        "Activity" | "SessionEndCause" | "HarnessKind" | "RemoteLaunchAccess" | "StandingRole" | "SessionStateView" | "AllowanceState" => {
            invalid.push(json!("unrecognizedVariant"));
            for literal in literals { if let Some(value) = literal.as_str() { invalid.push(json!(value.to_uppercase())); } }
        }
        "Heartbeat" => invalid.push(json!({"capabilities":["RemoteStart"]})),
        "ReportSessionActivity" | "MachinePresenceAnswer" => {
            if let Some(first) = literals.first() {
                let mut value = first.clone();
                let field = if name == "ReportSessionActivity" { "sequence" } else { "attachSequence" };
                value[field] = json!(9007199254740992_u64);
                invalid.push(value);
            }
        }
        "MachineEnrollment" => {
            if let Some(first) = literals.first() {
                for state in ["reserved","approved"] {
                    let mut value = first.clone(); value["state"] = json!(state); invalid.push(value);
                }
            }
            for literal in literals {
                let mut value = literal.clone();
                if let Some(fields) = value.as_object_mut() { fields.remove("machineClientId"); }
                invalid.push(value);
            }
        }
        "MachinePrincipal" => invalid.push(json!({"kind":"machine","installationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002"})),
        "MachineTokenClaims" => {
            if let Some(first) = literals.first() {
                let mut missing = first.clone();
                if let Some(fields) = missing.as_object_mut() { fields.remove("principal"); }
                invalid.push(missing);
                for kind in ["session","person","service","fourthKind"] {
                    let mut value = first.clone(); value["principal"]["kind"] = json!(kind); invalid.push(value);
                }
            }
        }
        _ => {}
    }
    invalid
}
