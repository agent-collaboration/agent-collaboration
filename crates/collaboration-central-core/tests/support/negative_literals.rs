use serde_json::{Value, json};

pub fn explicit_negatives(name: &str, literals: &[Value]) -> Vec<Value> {
    let mut invalid = vec![Value::Null];
    match name {
        "AuthorizationRevision" | "AuthorizationRevisionAcknowledgement" => {
            for literal in literals {
                for field in literal.as_object().unwrap().keys() {
                    let mut missing = literal.clone();
                    missing.as_object_mut().unwrap().remove(field);
                    invalid.push(missing);
                }
                let revision_field = if name == "AuthorizationRevision" {
                    "revision"
                } else {
                    "acknowledgedRevision"
                };
                for revision in [json!(0), json!(9_007_199_254_740_992_i64), json!(1.5), json!("4"), Value::Null] {
                    let mut value = literal.clone();
                    value[revision_field] = revision;
                    invalid.push(value);
                }
                if literal.get("permissionPolicyDigest").is_some() {
                    for digest in [json!(""), json!("a".repeat(63)), json!("a".repeat(65)), json!("A".repeat(64)), json!("g".repeat(64)), json!(42), Value::Null] {
                        let mut value = literal.clone();
                        value["permissionPolicyDigest"] = digest;
                        invalid.push(value);
                    }
                }
            }
        }
        "OrganizationAdminResult" => {
            let applied = literals.iter().find(|value| value["result"] == "applied").unwrap();
            let mut legacy = applied.clone();
            legacy["alreadyExisted"] = json!(false);
            invalid.push(legacy);
            let mut missing_description = applied.clone();
            missing_description["workspace"].as_object_mut().unwrap().remove("description");
            invalid.push(missing_description);
            invalid.extend([
                json!({"result":"alreadyExisted"}),
                json!({"result":"refused","reason":"unrecognizedVariant"}),
                json!({"result":"refused","reason":{"kind":"nameInUse"}}),
            ]);
        }
        "CreateWorkspaceRequest" => {
            let mut missing_description = literals[0].clone();
            missing_description.as_object_mut().unwrap().remove("description");
            invalid.push(missing_description);
            let mut padded_name = literals[0].clone();
            padded_name["name"] = json!(" Fixture workspace ");
            invalid.push(padded_name);
            let mut wrong_operation = literals[0].clone();
            wrong_operation["operationId"] = json!("00000000-0000-4000-8000-000000000001");
            invalid.push(wrong_operation);
        }
        "ReadChangeRecordsRequest" => {
            for limit in [json!(0), json!(101), json!(1.5)] {
                let mut wrong_limit = literals[0].clone();
                wrong_limit["limit"] = limit;
                invalid.push(wrong_limit);
            }
        }
        "OrganizationAdminIntent" => {
            for literal in literals {
                for version in [0, 2] {
                    let mut wrong_version = literal.clone();
                    wrong_version["v"] = json!(version);
                    invalid.push(wrong_version);
                }
                let mut wrong_identity = literal.clone();
                wrong_identity["operationId"] = if literal["operation"] == "CreateWorkspace" {
                    Value::Null
                } else {
                    json!("019a3dce-2000-7000-8000-000000000003")
                };
                invalid.push(wrong_identity);
                let mut wrong_params = literal.clone();
                wrong_params["params"] = if literal["operation"] == "WorkspaceLookup" {
                    json!({"name":"Fixture workspace","description":"Fixture description"})
                } else {
                    json!({})
                };
                invalid.push(wrong_params);
            }
        }
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
    if name == "DecideDeviceLogin" {
        let first = literals.first().expect("device-login literal");
        for field in [
            "permissionChecked",
            "approverSecondFactorAt",
            "codeExpiresAt",
        ] {
            let mut missing = first.clone();
            missing.as_object_mut().unwrap().remove(field);
            invalid.push(missing);
        }
        for (field, bad) in [
            ("permissionChecked", json!("machine:revoke")),
            (
                "installation",
                json!("00000000-0000-4000-8000-000000000001"),
            ),
            (
                "formerServiceId",
                json!("019a3dce-2000-7000-8000-000000000001"),
            ),
            ("codeExpiresAt", Value::Null),
            ("codeExpiresAt", json!(-1)),
            ("codeExpiresAt", json!(1793448600.5)),
            ("codeExpiresAt", json!("1793448600")),
            ("approverSecondFactorAt", json!(-1)),
            ("approverSecondFactorAt", json!(1.5)),
            ("approverSecondFactorAt", json!("1793448000")),
            (
                "allowedWorkspaces",
                json!(vec!["019a3dce-2000-7000-8000-000000000001"; 101]),
            ),
        ] {
            let mut value = first.clone();
            value[field] = bad;
            invalid.push(value);
        }
    }
    match name {
        "AllowedWorkspaces" => invalid.extend([
            json!(vec!["019a3dce-2000-7000-8000-000000000001"; 101]),
            json!(["00000000-0000-4000-8000-000000000001"]),
            json!(["019A3DCE-2000-7000-8000-000000000001"]),
        ]),
        "EnrollPermission" => invalid.extend([json!("machine:revoke"), json!("Machine:Enroll")]),
        "DeviceLoginRefusal" => invalid.extend([
            json!({"kind":"notOwner"}),
            json!({"kind":"formerServiceLinkedElsewhere"}),
            json!({"kind":"secondFactorRequired","withinSeconds":0}),
        ]),
        "DeviceLoginDecision" => invalid.extend([
            json!({"result":"approved","enrollmentId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"result":"refused","reason":{"kind":"notOwner"}}),
            json!({"result":"refused","reason":{"kind":"formerServiceLinkedElsewhere"}}),
            json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":0}}),
        ]),
        "MachineTokenDecision" => {
            let first = literals.first().expect("machine token grant literal");
            for field in ["decidedAt", "grantExpiresAt"] {
                for bad in [json!(-1), json!(1.5), json!("1793448600"), Value::Null] {
                    let mut value = first.clone();
                    value[field] = bad;
                    invalid.push(value);
                }
            }
        }
        _ => {}
    }
    invalid.extend(composed_byte_overflows(name, literals));
    invalid.extend(description_byte_overflows(name, literals));
    invalid.extend(scalar_rule_negatives(literals));
    invalid
}

fn description_byte_overflows(name: &str, literals: &[Value]) -> Vec<Value> {
    let pointer = match name {
        "CreateWorkspaceRequest" => "/description",
        "OrganizationAdminResult" => "/workspace/description",
        "OrganizationAdminIntent" => "/params/description",
        _ => return Vec::new(),
    };
    let mut invalid = Vec::new();
    for literal in literals
        .iter()
        .filter(|literal| literal.pointer(pointer).is_some())
    {
        // Multibyte values stay below maxLength while crossing the UTF-8 byte bound.
        for description in [
            "a".repeat(2049),
            format!("{}a", "é".repeat(1024)),
            format!("{}a", "😀".repeat(512)),
            "😀".repeat(513),
        ] {
            assert!(description.len() > 2048);
            let mut overflow = literal.clone();
            *overflow.pointer_mut(pointer).unwrap() = json!(description);
            invalid.push(overflow);
        }
    }
    invalid
}

// These inputs exceed the byte bound while staying below JSON Schema's character
// bound. Each owning contract must reject them through its registered keyword.
fn composed_byte_overflows(name: &str, literals: &[Value]) -> Vec<Value> {
    let fields: &[(&str, usize)] = match name {
        "DecideDeviceLogin" | "AllowedMachineRow" | "MachineLookupResult" => &[("/label", 256)],
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
