use super::*;
use serde_json::json;

#[test]
fn post_send_transport_loss_names_unknown_setting_state() {
    let request: ProviderSettingsSetRequest = serde_json::from_value(json!({
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"provider-session"},
            "actor":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"creator"},
            "setting":"mode","value":"ask"
        })).expect("set request");
    let failure = setting_error(&request, ExternalProviderRuntimeError::TransportFailure);
    assert_eq!(failure.kind, ProviderSettingsFailureKind::OutcomeUnknown);
    assert_eq!(failure.setting, Some(ProviderSettingName::Mode));
    assert_eq!(failure.value.as_deref(), Some("ask"));
    let precise = setting_error(
        &request,
        ExternalProviderRuntimeError::SettingOutcomeUnknown {
            provider_session_id: "provider-session".into(),
            setting: acp_client_runtime::ProviderSettingKind::Mode,
            value: "ask".into(),
        },
    );
    assert_eq!(precise.kind, ProviderSettingsFailureKind::OutcomeUnknown);
    assert_eq!(precise.setting, Some(ProviderSettingName::Mode));
}
