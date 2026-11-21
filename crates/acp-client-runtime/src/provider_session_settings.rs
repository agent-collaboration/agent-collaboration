//! Typed provider Session settings at the ACP client boundary.
use provider_session_machines::{EffectiveProviderSettings, ProviderSettingKind};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestedProviderSettings {
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: FastRequest,
}

impl RequestedProviderSettings {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.model.is_none()
            && self.effort.is_none()
            && self.fast == FastRequest::Leave
    }
}

/// What a request asks of the agent's fast option, and where it came from
/// (D1 spec R7, R10). The Router never turns fast on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FastRequest {
    /// Don't touch fast: `--fast auto`, and every setter that isn't about fast.
    #[default]
    Leave,
    /// The create default: turn fast off when the agent offers it; a no-op
    /// when it doesn't.
    OffIfOffered,
    /// An explicit fast off: with no fast option it's an invalid setting.
    Off,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSettingApplication {
    pub effective_settings: EffectiveProviderSettings,
    /// The requested mode confirmed by this application without a fallback.
    pub confirmed_requested_mode: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSettingRequest {
    pub kind: ProviderSettingKind,
    pub value: String,
    pub intent: session_event_model::SettingApplicationIntent,
    pub expected_model: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
    pub reason: session_event_model::ProviderSettingFailureReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotAppliedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidSettingSessionDisposition {
    Closed,
    RemainsCreated,
}
