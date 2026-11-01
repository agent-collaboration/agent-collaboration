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

/// Live layout presence, independent of whether a painted control can activate.
#[derive(Clone)]
pub struct TargetVisibility(Arc<dyn Fn() -> bool + Send + Sync>);
impl TargetVisibility {
    pub fn new(is_painted: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(is_painted))
    }
    pub fn is_painted(&self) -> bool {
        (self.0)()
    }
}
impl Default for TargetVisibility {
    fn default() -> Self {
        Self::new(|| true)
    }
}
impl fmt::Debug for TargetVisibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TargetVisibility")
    }
}

#[derive(Clone, Debug)]
pub struct TargetSpec {
    pub id: TargetId,
    pub role: TargetRole,
    pub group: Option<GroupId>,
    pub availability: AvailabilityFn,
    pub visibility: TargetVisibility,
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
            visibility: TargetVisibility::default(),
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
    pub fn with_visibility(mut self, visibility: TargetVisibility) -> Self {
        self.visibility = visibility;
        self
    }
    pub fn is_enabled(&self) -> bool {
        self.visibility.is_painted() && self.availability.get().is_available()
    }
    fn can_retain_focus(&self) -> bool {
        self.role == TargetRole::Focusable && self.visibility.is_painted()
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
    initial: Option<TargetId>,
    displaced: Option<TargetId>,
    focus_origin: FocusOrigin,
    input_cause: FocusInputCause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusInputCause {
    Keyboard,
    Pointer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusOrigin {
    Intent,
    AutomaticFallback,
}
impl FocusModel {
    pub(crate) fn empty() -> Self {
        Self {
            targets: Vec::new(),
            current: None,
            initial: None,
            displaced: None,
            focus_origin: FocusOrigin::Intent,
            input_cause: FocusInputCause::Keyboard,
        }
    }
    pub fn new(targets: Vec<TargetSpec>) -> Result<Self, FocusError> {
        validate_targets(&targets)?;
        let current = targets
            .iter()
            .find(|target| target.role == TargetRole::Focusable)
            .map(|target| target.id.clone());
        let mut model = Self {
            targets,
            initial: current.clone(),
            current,
            displaced: None,
            focus_origin: FocusOrigin::Intent,
            input_cause: FocusInputCause::Keyboard,
        };
        model.reconcile_eligibility();
        Ok(model)
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
            self.displaced = None;
            self.focus_origin = FocusOrigin::Intent;
            self.input_cause = FocusInputCause::Keyboard;
            true
        } else {
            false
        }
    }
    pub(crate) fn focus_from_pointer(&mut self, id: &TargetId) -> bool {
        let focused = self.focus(id);
        if focused {
            self.input_cause = FocusInputCause::Pointer;
        }
        focused
    }
    pub(crate) fn reveals_focus(&self) -> bool {
        self.input_cause == FocusInputCause::Keyboard
    }
    fn is_focus_eligible(&self, id: &TargetId) -> bool {
        self.targets
            .iter()
            .any(|target| target.id == *id && target.can_focus())
    }
    fn is_painted_focus(&self, id: &TargetId) -> bool {
        self.targets
            .iter()
            .any(|target| target.id == *id && target.can_retain_focus())
    }
    fn automatic_fallback(&self, is_deferred: &impl Fn(&TargetSpec) -> bool) -> Option<TargetId> {
        self.initial
            .as_ref()
            .filter(|id| self.is_focus_eligible(id))
            .cloned()
            .or_else(|| {
                self.targets
                    .iter()
                    .find(|target| target.can_focus() && !is_deferred(target))
                    .map(|target| target.id.clone())
            })
            .or_else(|| {
                self.targets
                    .iter()
                    .find(|target| target.can_focus())
                    .map(|target| target.id.clone())
            })
    }
    pub(crate) fn capture_initial_focus(&mut self) {
        if self.focus_origin == FocusOrigin::Intent {
            self.initial = self.current.clone().or_else(|| self.initial.clone());
        }
    }
    /// Reconcile only the active surface. Covered and Help-suspended models keep
    /// their displaced intent until the original surface becomes active again.
    pub(crate) fn reconcile_eligibility(&mut self) -> bool {
        self.reconcile_with_priority(|_| false)
    }
    pub(crate) fn reconcile_with_priority(
        &mut self,
        is_deferred: impl Fn(&TargetSpec) -> bool,
    ) -> bool {
        let before = (
            self.current.clone(),
            self.displaced.clone(),
            self.focus_origin,
        );
        if self
            .displaced
            .as_ref()
            .is_some_and(|id| self.is_focus_eligible(id))
        {
            self.current = self.displaced.take();
            self.focus_origin = FocusOrigin::Intent;
            self.input_cause = FocusInputCause::Keyboard;
        } else if !self
            .current
            .as_ref()
            .is_some_and(|id| self.is_painted_focus(id))
            || (self.focus_origin == FocusOrigin::AutomaticFallback
                && self
                    .current
                    .as_ref()
                    .is_some_and(|id| self.is_focus_eligible(id)))
        {
            if self.focus_origin == FocusOrigin::Intent && self.displaced.is_none() {
                self.displaced = self.current.clone();
            }
            self.current = self.automatic_fallback(&is_deferred);
            self.focus_origin = FocusOrigin::AutomaticFallback;
            self.input_cause = FocusInputCause::Keyboard;
        }
        (
            self.current.clone(),
            self.displaced.clone(),
            self.focus_origin,
        ) != before
    }
    /// Model replacement copies intent without reconciling a covered surface.
    pub(crate) fn inherit_focus_intent(&mut self, previous: &Self) {
        self.current = previous.current.clone();
        self.initial = previous.initial.clone();
        self.displaced = previous.displaced.clone();
        self.focus_origin = previous.focus_origin;
        self.input_cause = previous.input_cause;
    }
    pub fn restore(&mut self, id: Option<&TargetId>) {
        if self.current.as_ref() != id {
            self.focus_origin = FocusOrigin::Intent;
            self.input_cause = FocusInputCause::Keyboard;
        }
        self.current = id.cloned();
        self.reconcile_eligibility();
    }
    pub fn replace_targets(&mut self, targets: Vec<TargetSpec>) -> Result<(), FocusError> {
        validate_targets(&targets)?;
        self.targets = targets;
        self.reconcile_eligibility();
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
        let current_index = self
            .targets
            .iter()
            .position(|target| Some(&target.id) == self.current.as_ref());
        let eligible = self
            .targets
            .iter()
            .enumerate()
            .filter(|(_, target)| {
                target.can_focus() && (!within_group || target.group.as_ref() == group)
            })
            .collect::<Vec<_>>();
        let next = match direction {
            FocusDirection::Next => eligible
                .iter()
                .find(|(index, _)| current_index.is_some_and(|current| *index > current))
                .or_else(|| eligible.first()),
            FocusDirection::Previous => eligible
                .iter()
                .rev()
                .find(|(index, _)| current_index.is_some_and(|current| *index < current))
                .or_else(|| eligible.last()),
        };
        self.current = next.map(|(_, target)| target.id.clone());
        self.displaced = None;
        self.focus_origin = FocusOrigin::Intent;
        self.input_cause = FocusInputCause::Keyboard;
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

#[cfg(test)]
#[path = "disabled_control_focus_test.rs"]
mod disabled_control_test;
#[cfg(test)]
#[path = "focus_replacement_test.rs"]
mod replacement_test;
#[cfg(test)]
#[path = "focus_return_test.rs"]
mod return_test;
#[cfg(test)]
#[path = "focus_model_test.rs"]
mod tests;
