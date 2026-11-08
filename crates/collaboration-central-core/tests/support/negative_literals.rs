use serde_json::{Value, json};

pub fn explicit_negatives(name: &str, literals: &[Value]) -> Vec<Value> {
    let mut invalid = vec![Value::Null];
    match name {
        "ProvisionOrganization" => invalid.extend([
            json!({"operation":"admitSignIn","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002","verifiedEmail":"fixture@example.invalid"}),
            json!({"operation":"admitSignIn","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"operation":"standing","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002"}),
        ]),
        "ProvisionOrganizationResult" => invalid.extend([
            json!({"result":"admitted","role":"member"}),
            json!({"result":"admitted","role":"owner"}),
            json!({"result":"awaitingOwner"}),
            json!({"result":"notAdmitted"}),
            json!({"result":"refused","reason":{"kind":"notPermitted"}}),
        ]),
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
        "MachineLabel" => invalid.extend([json!(format!("{}a", "é".repeat(128))), json!("a\0b"), json!("a\rb"), json!(" \t\n"), json!("\u{0085}"), json!("\u{0085}\u{2003}")]),
        "InstallationId" => invalid.extend([json!("x".repeat(36)), json!("019A3DCE-2000-7000-8000-000000000001"), json!("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"), json!("00000000-0000-4000-8000-000000000001\n")]),
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
    invalid.extend(composed_byte_overflows(name, literals));
    invalid.extend(scalar_rule_negatives(literals));
    invalid.extend(machine_login_mirror_negatives(name, literals));
    invalid
}

/// Inputs the issuer's `machine-login-contracts.ts` refuses; central refuses them too.
fn machine_login_mirror_negatives(name: &str, literals: &[Value]) -> Vec<Value> {
    let mut invalid = Vec::new();
    match name {
        "DecideDeviceLogin" => {
            let Some(first) = literals.first() else { return invalid };
            let mut missing = first.clone();
            if let Some(fields) = missing.as_object_mut() { fields.remove("approverSecondFactorAt"); }
            invalid.push(missing);
            let mut no_permission = first.clone();
            if let Some(fields) = no_permission.as_object_mut() { fields.remove("permissionChecked"); }
            invalid.push(no_permission);
            let cases: [(&str, Value); 12] = [
                ("permissionChecked", json!("machine:view")),
                ("label", json!("a".repeat(121))),
                // 61 astral characters: 61 code points, 122 UTF-16 code units.
                ("label", json!("😀".repeat(61))),
                // 86 three-byte characters: 86 code units, 258 UTF-8 bytes.
                ("label", json!("€".repeat(86))),
                ("label", json!("")),
                ("allowedWorkspaces", json!(vec!["019a3dce-2000-7000-8000-000000000001"; 101])),
                ("approverSecondFactorAt", json!("1793448000")),
                ("formerServiceId", json!("019a3dce-2000-1000-8000-000000000001")),
                ("formerServiceId", json!("019a3dce-2000-5000-8000-000000000001")),
                ("formerServiceId", json!("00000000-0000-4000-0000-000000000001")),
                ("formerServiceId", json!("00000000-0000-4000-8000-00000000000G")),
                ("formerServiceId", json!(17)),
            ];
            for (field, value) in cases {
                let mut changed = first.clone();
                changed[field] = value;
                invalid.push(changed);
            }
        }
        "DeviceLoginRefusal" => invalid.extend([
            json!({"kind":"notOwner"}),
            json!({"kind":"secondFactorRequired","withinSeconds":0}),
            json!({"kind":"secondFactorRequired"}),
        ]),
        "DeviceLoginDecision" => invalid.extend([
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
            json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":0}}),
        ]),
        "MachineTokenDecision" => invalid.extend([
            json!({"result":"grant","installation":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","decidedAt":1793448000_i64,"grantExpiresAt":1793448600_i64,"superseded":{"enrollmentId":"019a3dce-2000-7000-8000-000000000003"}}),
            json!({"result":"grant","installation":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002","decidedAt":1793448000_i64,"grantExpiresAt":1793448600_i64,"superseded":{"clientId":"019a3dce-2000-7000-8000-000000000004"}}),
        ]),
        _ => {}
    }
    invalid
}

// These inputs exceed the byte bound while staying below JSON Schema's character
// bound. Each owning contract must reject them through its registered keyword.
fn composed_byte_overflows(name: &str, literals: &[Value]) -> Vec<Value> {
    let fields: &[(&str, usize)] = match name {
        "AllowedMachineRow" | "MachineLookupResult" => &[("/label", 256)],
        "CreateWorkspace" | "ProjectRef" | "WorkspaceRef" => &[("/name", 256)],
        "IdentityLookup" | "RemoveSession" | "SessionRow" => &[("/session/sessionId", 4096)],
        "MachineRow" => &[("/label", 256), ("/sessions/0/session/sessionId", 4096)],
        "WhoIsLoggedInResult" => &[
            ("/machines/0/label", 256),
            ("/machines/0/sessions/0/session/sessionId", 4096),
        ],
        "IdentityLookupResult" => &[("/launchContext/workingDirectory", 4096)],
        "WorkspaceLookupResult" => &[("/workspace/name", 256), ("/allowedMachines/0/label", 256)],
        _ => &[],
    };
    fields
        .iter()
        .map(|(pointer, maximum_bytes)| {
            let mut invalid = literals
                .iter()
                .find(|literal| literal.pointer(pointer).is_some())
                .unwrap_or_else(|| panic!("{name} has no literal with {pointer}"))
                .clone();
            let overflow = if *pointer == "/launchContext/workingDirectory" {
                format!("/{}", "é".repeat(maximum_bytes / 2))
            } else {
                format!("{}a", "é".repeat(maximum_bytes / 2))
            };
            assert_eq!(overflow.len(), maximum_bytes + 1);
            *invalid.pointer_mut(pointer).unwrap() = json!(overflow);
            invalid
        })
        .collect()
}

// The review's malformed scalars must remain rejected in every composed wire shape.
fn scalar_rule_negatives(literals: &[Value]) -> Vec<Value> {
    fn collect(value: &Value, path: &str, cases: &mut Vec<(String, Value)>) {
        match value {
            Value::Object(fields) => {
                for (field, child) in fields {
                    let pointer = format!("{path}/{}", field.replace('~', "~0").replace('/', "~1"));
                    let invalid = match field.as_str() {
                        "installation" | "installationId" | "installation_id" | "serviceId" => {
                            vec![
                                json!("x".repeat(36)),
                                json!("019A3DCE-2000-7000-8000-000000000001"),
                                json!("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"),
                            ]
                        }
                        "endpointId" => vec![json!("Codex"), json!("9"), json!("codex-local\n")],
                        "sessionId" => vec![json!("a\0b")],
                        "name" => vec![
                            json!(" "),
                            json!("\t\n"),
                            json!("\u{0085}"),
                            json!("\u{2003}"),
                        ],
                        "label" => vec![
                            json!("a\0b"),
                            json!("a\rb"),
                            json!(" \t\n"),
                            json!("\u{0085}"),
                            json!("\u{0085}\u{2003}"),
                        ],
                        _ => Vec::new(),
                    };
                    cases.extend(
                        invalid
                            .into_iter()
                            .map(|invalid| (pointer.clone(), invalid)),
                    );
                    collect(child, &pointer, cases);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    collect(child, &format!("{path}/{index}"), cases);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    let mut negatives = Vec::new();
    for literal in literals {
        let mut cases = Vec::new();
        collect(literal, "", &mut cases);
        for (pointer, invalid) in cases {
            let mut negative = literal.clone();
            *negative.pointer_mut(&pointer).unwrap() = invalid;
            negatives.push(negative);
        }
    }
    negatives
}
