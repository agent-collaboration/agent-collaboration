//! Native-only absolute references for provider-owned skill invocations.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use message_board::SessionRef;
use session_event_model::{SelectedSkill, SkillInvocationName};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("skill reference does not identify the addressed session and selected name")]
pub(crate) struct InvalidNativeSkillReference;

pub(crate) fn encode_skill_reference(session: &SessionRef, name: &SkillInvocationName) -> String {
    format!(
        "/__codex_router_skill__/{}/{}/{}",
        session.endpoint.endpoint_id.as_str(),
        URL_SAFE_NO_PAD.encode(session.session_id.as_str()),
        URL_SAFE_NO_PAD.encode(name.as_str())
    )
}

pub(crate) fn decode_skill_reference(
    session: &SessionRef,
    structured_name: &str,
    reference: &str,
) -> Result<SelectedSkill, InvalidNativeSkillReference> {
    let components = reference.split('/').collect::<Vec<_>>();
    let [
        "",
        "__codex_router_skill__",
        endpoint,
        encoded_session,
        encoded_name,
    ] = components.as_slice()
    else {
        return Err(InvalidNativeSkillReference);
    };
    if *endpoint != session.endpoint.endpoint_id.as_str() {
        return Err(InvalidNativeSkillReference);
    }
    let decoded_session = decode_component(encoded_session)?;
    let decoded_name = decode_component(encoded_name)?;
    if decoded_session != session.session_id.as_str() || decoded_name != structured_name {
        return Err(InvalidNativeSkillReference);
    }
    Ok(SelectedSkill {
        invocation_name: SkillInvocationName::new(decoded_name)
            .map_err(|_| InvalidNativeSkillReference)?,
    })
}

fn decode_component(encoded: &str) -> Result<String, InvalidNativeSkillReference> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| InvalidNativeSkillReference)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(InvalidNativeSkillReference);
    }
    String::from_utf8(bytes).map_err(|_| InvalidNativeSkillReference)
}

#[cfg(test)]
mod tests {
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
}
