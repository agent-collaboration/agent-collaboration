//! Definitions are shared by routing and every action projection.
use crate::foundation::KeyChord;
use iocraft::{Handler, KeyEvent};
use std::{fmt, sync::Arc};
mod action_title;
pub use action_title::ActionTitle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentifierError;
impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an identity must contain non-whitespace text")
    }
}
impl std::error::Error for IdentifierError {}

pub(crate) fn validate_identifier(value: &str) -> Result<(), IdentifierError> {
    if value.trim().is_empty() {
        Err(IdentifierError)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ActionId(String);
impl ActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ScopeId(String);
impl ScopeId {
    pub(crate) fn screen() -> Self {
        Self("screen".into())
    }
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
pub enum Availability {
    Available,
    Unavailable { reason: String },
}
impl Availability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Clone)]
pub struct AvailabilityFn(Arc<dyn Fn() -> Availability + Send + Sync>);
impl AvailabilityFn {
    pub fn new(callback: impl Fn() -> Availability + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
    pub fn get(&self) -> Availability {
        (self.0)()
    }
}
impl Default for AvailabilityFn {
    fn default() -> Self {
        Self::new(|| Availability::Available)
    }
}
impl fmt::Debug for AvailabilityFn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AvailabilityFn")
    }
}

#[derive(Clone)]
pub struct Checked(Arc<dyn Fn() -> bool + Send + Sync>);
impl Checked {
    pub fn new(callback: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
    pub fn get(&self) -> bool {
        (self.0)()
    }
}
impl fmt::Debug for Checked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Checked")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Placement {
    Toolbar,
    Menu { section: &'static str },
    Hidden,
}

/// Ordered menu metadata; the referenced action owns the live heading and key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuSectionSpec {
    pub section: &'static str,
    pub heading_action: Option<ActionId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Declared effect risk used to prioritize automatic focus, without invoking an action.
pub enum ActionRisk {
    #[default]
    Ordinary,
    Destructive,
}

#[derive(Clone)]
pub struct ActionDef {
    pub id: ActionId,
    pub title: ActionTitle,
    /// Only buttons may use this short label; other projections keep the title.
    pub toolbar_label: Option<ActionTitle>,
    pub keys: Vec<KeyChord>,
    pub availability: AvailabilityFn,
    pub choice: Option<Checked>,
    pub placement: Placement,
    pub risk: ActionRisk,
    pub run: Handler<()>,
}
impl ActionDef {
    pub(crate) fn presentation(&self) -> ActionPresentation {
        ActionPresentation {
            id: self.id.clone(),
            title: self.title.get(),
            toolbar_label: self.toolbar_label.as_ref().map(ActionTitle::get),
            keys: self.keys.clone(),
            availability: self.availability.get(),
            checked: self.choice.as_ref().map(Checked::get),
            placement: self.placement.clone(),
        }
    }
}
impl fmt::Debug for ActionDef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActionDef")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("toolbar_label", &self.toolbar_label)
            .field("keys", &self.keys)
            .field("availability", &self.availability)
            .field("choice", &self.choice)
            .field("placement", &self.placement)
            .field("risk", &self.risk)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionPresentation {
    pub id: ActionId,
    pub title: String,
    pub toolbar_label: Option<String>,
    pub keys: Vec<KeyChord>,
    pub availability: Availability,
    pub checked: Option<bool>,
    pub placement: Placement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionError {
    DuplicateId(ActionId),
    DuplicateKey(KeyChord),
    BlankMenuSection,
    DuplicateMenuSection(&'static str),
    MissingMenuHeading(ActionId),
    DuplicateMenuHeading(ActionId),
}
impl fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid action scope: {self:?}")
    }
}
impl std::error::Error for ActionError {}

#[derive(Clone, Debug)]
pub struct ActionRegistry {
    scope: ScopeId,
    actions: Vec<ActionDef>,
    menu_sections: Vec<MenuSectionSpec>,
}
impl ActionRegistry {
    pub(crate) fn empty() -> Self {
        Self {
            scope: ScopeId::screen(),
            actions: Vec::new(),
            menu_sections: Vec::new(),
        }
    }
    pub fn new(scope: ScopeId, actions: Vec<ActionDef>) -> Result<Self, ActionError> {
        let mut identities = std::collections::HashSet::new();
        let mut bindings = std::collections::HashSet::new();
        for action in &actions {
            if !identities.insert(action.id.clone()) {
                return Err(ActionError::DuplicateId(action.id.clone()));
            }
            for key in &action.keys {
                if !bindings.insert(key.clone()) {
                    return Err(ActionError::DuplicateKey(key.clone()));
                }
            }
        }
        Ok(Self {
            scope,
            actions,
            menu_sections: Vec::new(),
        })
    }
    pub fn with_menu_sections(
        mut self,
        sections: Vec<MenuSectionSpec>,
    ) -> Result<Self, ActionError> {
        let mut names = std::collections::HashSet::new();
        let mut headings = std::collections::HashSet::new();
        for section in &sections {
            if section.section.trim().is_empty() {
                return Err(ActionError::BlankMenuSection);
            }
            if !names.insert(section.section) {
                return Err(ActionError::DuplicateMenuSection(section.section));
            }
            if let Some(id) = &section.heading_action {
                if !self.actions.iter().any(|action| action.id == *id) {
                    return Err(ActionError::MissingMenuHeading(id.clone()));
                }
                if !headings.insert(id) {
                    return Err(ActionError::DuplicateMenuHeading(id.clone()));
                }
            }
        }
        self.menu_sections = sections;
        Ok(self)
    }
    pub(crate) fn menu_sections(&self) -> &[MenuSectionSpec] {
        &self.menu_sections
    }
    pub fn scope(&self) -> &ScopeId {
        &self.scope
    }
    pub fn definitions(&self) -> &[ActionDef] {
        &self.actions
    }
    pub fn invocation(&self, id: &ActionId) -> Option<Handler<()>> {
        self.actions
            .iter()
            .find(|action| action.id == *id)
            .filter(|action| action.availability.get().is_available())
            .map(|action| action.run.clone())
    }
    pub fn key_invocation(&self, event: &KeyEvent) -> Option<(ActionId, Handler<()>)> {
        self.actions
            .iter()
            .rev()
            .find(|action| action.keys.iter().any(|key| key.matches(event)))
            .and_then(|action| {
                self.invocation(&action.id)
                    .map(|handler| (action.id.clone(), handler))
            })
    }
    pub fn presentations(&self) -> Vec<ActionPresentation> {
        self.actions.iter().map(ActionDef::presentation).collect()
    }
}

#[cfg(test)]
#[path = "actions_test.rs"]
mod tests;

#[cfg(test)]
#[path = "menu_sections_test.rs"]
mod menu_sections_test;
