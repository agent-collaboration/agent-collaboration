use serde_json::{Value, json};

pub fn accepted_reads(name: &str, literals: &[Value]) -> Vec<Value> {
    let mut reads = Vec::new();
    for literal in literals {
        let fields: &[&str] = match name {
            "LaunchContext" => &["repository", "commit", "model"],
            "DecideDeviceLogin" => &["approverSecondFactorAt"],
            "PersonAdmissionRequest" if literal["operation"] == "admitSignIn" => &["verifiedEmail"],
            "MachinePresenceAnswer" if literal["state"] == "notConnected" => &["closedAgoMs"],
            "ConnectionView" if literal["state"] == "notConnected" => &["lastSeenAt"],
            "MachineEnrollment" => &["endsAt"],
            _ => &[],
        };
        for &field in fields {
            if literal.get(field).is_none() {
                let mut input = literal.clone();
                input[field] = Value::Null;
                reads.push(json!({"input":input,"canonical":literal}));
            }
        }
        if name == "MachineTokenClaims" {
            let mut input = literal.clone();
            input["iss"] = json!("https://issuer.example.invalid");
            input["sub"] = json!("owner-account");
            input["aud"] = json!("https://central.example.invalid/machine");
            input["exp"] = json!(1793448600);
            input["iat"] = json!(1793448000);
            input["jti"] = json!("019a3dce-2000-7000-8000-000000000001");
            input["client_id"] = json!("019a3dce-2000-7000-8000-000000000002");
            reads.push(json!({"input":input,"canonical":literal}));
        }
    }
    reads
}
