use super::*;

fn session(id: &str, endpoint: &str) -> Result<SessionRef, serde_json::Error> {
    serde_json::from_value(
        serde_json::json!({"endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":endpoint},"sessionId":id}),
    )
}

#[test]
fn exact_reference_round_trip_encodes_legal_session_and_invocation_components() {
    let target = session("/../会話/session", "claude-local").expect("target");
    let name = SkillInvocationName::new("plugin:技能/../review").expect("name");
    let path = encode_skill_reference(&target, &name);
    assert_eq!(path.split('/').count(), 5);
    assert_eq!(
        decode_skill_reference(&target, name.as_str(), &path),
        Ok(SelectedSkill {
            invocation_name: name
        })
    );
}

#[test]
fn foreign_same_name_wrong_structured_name_and_noncanonical_paths_refuse() {
    let target = session("target", "claude-local").expect("target");
    let name = SkillInvocationName::new("review").expect("name");
    let valid = encode_skill_reference(&target, &name);
    for path in [
        encode_skill_reference(&session("foreign", "claude-local").expect("foreign"), &name),
        encode_skill_reference(
            &session("target", "cursor-local").expect("foreign endpoint"),
            &name,
        ),
        format!("{valid}/extra"),
        format!("{valid}="),
        valid.replace("/__codex_router_skill__/", "/__codex_router_skill__//"),
    ] {
        assert!(decode_skill_reference(&target, "review", &path).is_err());
    }
    assert!(decode_skill_reference(&target, "other", &valid).is_err());
    assert!(
        decode_skill_reference(
            &target,
            "review",
            "/__codex_router_skill__/claude-local/dGFyZ2V0/_w"
        )
        .is_err()
    );
}
