//! Stable choice identity is independent of row position and pickability.
use crate::actions::{Availability, IdentifierError, validate_identifier};
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OptionKey(String);
impl OptionKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectOption {
    pub key: OptionKey,
    pub label: String,
    pub secondary: Option<String>,
    pub availability: Availability,
}
impl SelectOption {
    pub fn available(key: OptionKey, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            secondary: None,
            availability: Availability::Available,
        }
    }
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.label.to_lowercase().contains(&query)
            || self
                .secondary
                .as_ref()
                .is_some_and(|label| label.to_lowercase().contains(&query))
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateOptionKey(pub OptionKey);
impl std::fmt::Display for DuplicateOptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "duplicate option key: {}", self.0.as_str())
    }
}
impl std::error::Error for DuplicateOptionKey {}
pub(crate) fn validate_options(options: &[SelectOption]) -> Result<(), DuplicateOptionKey> {
    let mut keys = std::collections::HashSet::new();
    for option in options {
        if !keys.insert(&option.key) {
            return Err(DuplicateOptionKey(option.key.clone()));
        }
    }
    Ok(())
}
