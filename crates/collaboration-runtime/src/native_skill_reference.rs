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
#[path = "native_skill_reference_test.rs"]
mod tests;
