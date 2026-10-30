//! Ordered traversal adapted from CometixTUI focus.rs, MIT, ec7c0615.
//! Identity and groups replace its render-time registration and sibling scopes.
use crate::actions::{AvailabilityFn, IdentifierError, validate_identifier};
use iocraft::TerminalEvent;
use std::sync::Arc;
use std::{collections::HashSet, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputUse {
    Unused,
    Used,
    Scrolled,
}
#[derive(Clone)]
pub struct TargetInputHandler(Arc<dyn Fn(TerminalEvent) -> InputUse + Send + Sync>);
impl TargetInputHandler {
    pub fn new(callback: impl Fn(TerminalEvent) -> InputUse + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
    pub fn call(&self, event: TerminalEvent) -> InputUse {
        (self.0)(event)
    }
}
impl Default for TargetInputHandler {
    fn default() -> Self {
        Self::new(|_| InputUse::Unused)
    }
}
impl fmt::Debug for TargetInputHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TargetInputHandler")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TargetId(String);
impl TargetId {
    pub fn for_action(action: &crate::actions::ActionId) -> Self {
        Self(format!("action:{}", action.as_str()))
    }
    pub fn child(&self, suffix: &str) -> Self {
        Self(format!("{}:{}:{suffix}", self.0.len(), self.0))
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupId(String);
impl GroupId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetRole {
    Focusable,
    PointerOnly,
}

#[derive(Clone, Debug)]
pub struct TargetSpec {
    pub id: TargetId,
    pub role: TargetRole,
    pub group: Option<GroupId>,
    pub availability: AvailabilityFn,
    pub input: TargetInputHandler,
    pub activation: Option<crate::actions::ActionId>,
}
impl TargetSpec {
    pub fn new(id: TargetId) -> Self {
        Self {
            id,
            role: TargetRole::Focusable,
            group: None,
            availability: AvailabilityFn::default(),
            input: TargetInputHandler::default(),
            activation: None,
        }
    }
    pub fn in_group(mut self, group: GroupId) -> Self {
        self.group = Some(group);
        self
    }
    pub fn pointer_only(mut self) -> Self {
        self.role = TargetRole::PointerOnly;
        self
    }
    pub fn can_focus(&self) -> bool {
        self.role == TargetRole::Focusable && self.is_enabled()
    }
    pub fn with_input(mut self, input: TargetInputHandler) -> Self {
        self.input = input;
        self
    }
    pub fn with_activation(mut self, action: crate::actions::ActionId) -> Self {
        self.activation = Some(action);
        self
    }
    pub fn with_availability(mut self, availability: AvailabilityFn) -> Self {
        self.availability = availability;
        self
    }
    pub fn is_enabled(&self) -> bool {
        self.availability.get().is_available()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusDirection {
    Next,
    Previous,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusError {
    pub duplicate: TargetId,
}
impl fmt::Display for FocusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "duplicate focus target: {}",
            self.duplicate.as_str()
        )
    }
}
impl std::error::Error for FocusError {}

#[derive(Clone, Debug)]
pub struct FocusModel {
    targets: Vec<TargetSpec>,
    current: Option<TargetId>,
}
impl FocusModel {
    pub(crate) fn empty() -> Self {
        Self {
            targets: Vec::new(),
            current: None,
        }
    }
    pub fn new(targets: Vec<TargetSpec>) -> Result<Self, FocusError> {
        validate_targets(&targets)?;
        let current = targets
            .iter()
            .find(|target| target.can_focus())
            .map(|target| target.id.clone());
        Ok(Self { targets, current })
    }
    pub fn targets(&self) -> &[TargetSpec] {
        &self.targets
    }
    pub fn current(&self) -> Option<&TargetId> {
        self.current.as_ref()
    }
    pub fn focused(&self) -> Option<&TargetSpec> {
        self.current
            .as_ref()
            .and_then(|id| self.targets.iter().find(|target| target.id == *id))
    }
    pub fn focus(&mut self, id: &TargetId) -> bool {
        if self
            .targets
            .iter()
            .any(|target| target.id == *id && target.can_focus())
        {
            self.current = Some(id.clone());
            true
        } else {
            false
        }
    }
    pub fn restore(&mut self, id: Option<&TargetId>) {
        self.current = id
            .and_then(|id| {
                self.targets
                    .iter()
                    .find(|target| target.id == *id && target.can_focus())
            })
            .or_else(|| self.targets.iter().find(|target| target.can_focus()))
            .map(|target| target.id.clone());
    }
    pub fn replace_targets(&mut self, targets: Vec<TargetSpec>) -> Result<(), FocusError> {
        validate_targets(&targets)?;
        let previous = self.current.clone();
        self.targets = targets;
        self.restore(previous.as_ref());
        Ok(())
    }
    pub fn traverse(&mut self, direction: FocusDirection) {
        self.move_focus(direction, false);
    }
    pub fn traverse_group(&mut self, direction: FocusDirection) {
        self.move_focus(direction, true);
    }
    fn move_focus(&mut self, direction: FocusDirection, within_group: bool) {
        let group = self.focused().and_then(|target| target.group.as_ref());
        if within_group && group.is_none() {
            return;
        }
        let eligible = self
            .targets
            .iter()
            .filter(|target| {
                target.can_focus() && (!within_group || target.group.as_ref() == group)
            })
            .map(|target| &target.id)
            .collect::<Vec<_>>();
        let next = if let Some(index) = eligible
            .iter()
            .position(|id| Some(*id) == self.current.as_ref())
        {
            match direction {
                FocusDirection::Next => eligible.get(index + 1).or_else(|| eligible.first()),
                FocusDirection::Previous => index
                    .checked_sub(1)
                    .and_then(|index| eligible.get(index))
                    .or_else(|| eligible.last()),
            }
        } else {
            match direction {
                FocusDirection::Next => eligible.first(),
                FocusDirection::Previous => eligible.last(),
            }
        };
        self.current = next.map(|id| (*id).clone());
    }
}

fn validate_targets(targets: &[TargetSpec]) -> Result<(), FocusError> {
    let mut seen = HashSet::new();
    for target in targets {
        if !seen.insert(&target.id) {
            return Err(FocusError {
                duplicate: target.id.clone(),
            });
        }
    }
    Ok(())
}

/// Pure measured-coordinate conversion borrowed from CometixTUI scroll_view.rs.
pub fn scroll_content_top_for_absolute_rect(
    current_offset: i32,
    viewport_top: i32,
    target_top: i32,
    offset: i32,
) -> i32 {
    current_offset
        .saturating_add(target_top.saturating_sub(viewport_top))
        .saturating_add(offset)
}
