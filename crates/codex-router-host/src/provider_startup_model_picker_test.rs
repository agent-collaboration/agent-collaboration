use super::*;

#[test]
fn only_cursor_requests_parameterized_model_picker() {
    assert_eq!(
        provider_model_picker(ProviderKind::Cursor),
        acp_client_runtime::ProviderModelPicker::CursorParameterized
    );
    assert_eq!(
        provider_model_picker(ProviderKind::ClaudeCode),
        acp_client_runtime::ProviderModelPicker::Standard
    );
}
