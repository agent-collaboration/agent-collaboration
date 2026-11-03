//! Typed picker choices keep domain keys until the ComboBox binding projects them.
use agent_collaboration_tui::actions::Availability;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PickerChoice<TKey> {
    pub key: TKey,
    pub label: String,
    pub secondary: Option<String>,
    pub availability: Availability,
}

impl<TKey> PickerChoice<TKey> {
    pub fn available(key: TKey, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            secondary: None,
            availability: Availability::Available,
        }
    }
}
