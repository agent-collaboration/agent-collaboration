//! One advertised select value and its provider label.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSettingChoice {
    pub value: String,
    pub label: String,
}
