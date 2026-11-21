//! Compile-level contract consumed by the Host's provider create projection.

use acp_client_runtime::{
    AppliedProviderSetting, ExternalProviderCreatedSession, RequestedProviderSettings,
};
use provider_session_machines::EffectiveProviderSettings;
use provider_session_machines::ProviderSettingKind;

/// Oracle: specification R14 reports the effective settings and every applied
/// selection separately from a partially failed create.
#[test]
fn provider_create_settings_have_typed_request_and_result() {
    let requested = RequestedProviderSettings {
        mode: Some("ask".to_owned()),
        model: Some("model-a".to_owned()),
        effort: Some("high".to_owned()),
        ..Default::default()
    };
    let effective = EffectiveProviderSettings {
        mode: requested.mode,
        model: requested.model,
        effort: requested.effort,
        ..Default::default()
    };
    let created = ExternalProviderCreatedSession {
        provider_session_id: "session-a".to_owned(),
        effective_settings: effective.clone(),
        permission_mode: acp_client_runtime::PermissionModeReport::default(),
    };
    let applied = AppliedProviderSetting {
        kind: ProviderSettingKind::Mode,
        value: "ask".to_owned(),
    };

    assert_eq!(created.effective_settings, effective);
    assert_eq!(applied.kind, ProviderSettingKind::Mode);
}
