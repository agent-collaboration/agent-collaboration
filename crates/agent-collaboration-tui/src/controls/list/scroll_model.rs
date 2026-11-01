//! A scroll model can be constructed with its overlay before any component mounts.
use crate::surfaces::{
    focus_model::{InputUse, TargetId, TargetInputHandler},
    surface_handle::SurfaceStackHandle,
};
use iocraft::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollPosition {
    pub offset: u16,
    pub natural_height: u16,
    pub viewport_height: u16,
    pub viewport_top: i16,
}
impl ScrollPosition {
    pub fn maximum(self) -> u16 {
        self.natural_height.saturating_sub(self.viewport_height)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScrollRevealTarget {
    pub target_id: Option<TargetId>,
    pub logical_top: i32,
    pub height: u16,
    pub viewport_height: u16,
}
#[derive(Clone)]
pub struct ScrollHandle {
    position: Arc<Mutex<ScrollPosition>>,
    last_reveal: Arc<Mutex<Option<ScrollRevealTarget>>>,
    measured_targets: Arc<Mutex<HashMap<TargetId, ScrollRevealTarget>>>,
    pending_reveal: Arc<Mutex<Option<TargetId>>>,
    stack: SurfaceStackHandle,
}
impl ScrollHandle {
    pub fn new(stack: SurfaceStackHandle) -> Self {
        Self {
            position: Arc::new(Mutex::new(ScrollPosition::default())),
            last_reveal: Arc::new(Mutex::new(None)),
            measured_targets: Arc::new(Mutex::new(HashMap::new())),
            pending_reveal: Arc::new(Mutex::new(None)),
            stack,
        }
    }
    pub fn position(&self) -> ScrollPosition {
        *self
            .position
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn reset(&self) {
        *self
            .pending_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        *self
            .last_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.update(|position| position.offset = 0);
    }
    fn update(&self, update: impl FnOnce(&mut ScrollPosition)) -> bool {
        let (changed, scrolled) = {
            let mut position = self
                .position
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let before = *position;
            update(&mut position);
            position.offset = position.offset.min(position.maximum());
            (*position != before, position.offset != before.offset)
        };
        if scrolled {
            self.stack.hit_regions().invalidate();
        }
        if changed {
            self.stack.update(|_| {});
        }
        scrolled
    }
    pub(crate) fn measure_viewport(&self, height: u16, top: i16) {
        self.update(|position| {
            position.viewport_height = height;
            position.viewport_top = top;
        });
    }
    pub(crate) fn measure_content(&self, height: u16) {
        // Each draw replaces the derived geometry, including offscreen rows.
        self.measured_targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.update(|position| position.natural_height = height);
    }
    pub(super) fn measure_target(&self, target: ScrollRevealTarget) {
        if let Some(target_id) = target.target_id.clone() {
            self.measured_targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(target_id, target);
        }
        self.apply_pending_reveal();
    }
    /// Reveal with drawn geometry, retaining unmeasured intent until that target appears.
    pub(crate) fn reveal_measured(&self, target_id: &TargetId) {
        *self
            .pending_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(target_id.clone());
        self.apply_pending_reveal();
    }
    fn apply_pending_reveal(&self) {
        let position = self.position();
        if position.viewport_height == 0 || position.natural_height == 0 {
            return;
        }
        let mut pending = self
            .pending_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let target = pending.as_ref().and_then(|target_id| {
            self.measured_targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(target_id)
                .filter(|target| target.height > 0)
                .cloned()
        });
        let Some(target) = target else {
            return;
        };
        *pending = None;
        drop(pending);
        // New explicit input must reveal a target displaced by manual scrolling.
        self.forget_reveal(target.clone());
        self.reveal(target);
    }
    pub(super) fn forget_reveal(&self, target: ScrollRevealTarget) {
        let mut previous = self
            .last_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if previous.as_ref() == Some(&target) {
            *previous = None;
        }
    }
    pub(super) fn reveal(&self, target: ScrollRevealTarget) {
        let logical_top = target.logical_top;
        let height = target.height;
        {
            let mut previous = self
                .last_reveal
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if previous.as_ref() == Some(&target) {
                return;
            }
            *previous = Some(target);
        }
        self.update(|position| {
            let top = i32::from(position.offset);
            let bottom = top + i32::from(position.viewport_height);
            let target_bottom = logical_top + i32::from(height);
            let next = if logical_top < top {
                logical_top
            } else if target_bottom > bottom {
                if height > position.viewport_height {
                    logical_top
                } else {
                    target_bottom - i32::from(position.viewport_height)
                }
            } else {
                top
            };
            position.offset = u16::try_from(next.max(0)).unwrap_or(u16::MAX);
        });
    }
    pub fn handle_event(&self, event: &TerminalEvent) -> InputUse {
        let code = match event {
            TerminalEvent::Key(key) => match navigation_code(key) {
                Some(code) => code,
                None => return InputUse::Unused,
            },
            TerminalEvent::FullscreenMouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => KeyCode::Down,
                MouseEventKind::ScrollUp => KeyCode::Up,
                _ => return InputUse::Unused,
            },
            _ => return InputUse::Unused,
        };
        let scrolled = self.update(|position| {
            position.offset = match code {
                KeyCode::Up => position.offset.saturating_sub(1),
                KeyCode::Down => position.offset.saturating_add(1),
                KeyCode::PageUp => position
                    .offset
                    .saturating_sub(position.viewport_height.max(1)),
                KeyCode::PageDown => position
                    .offset
                    .saturating_add(position.viewport_height.max(1)),
                KeyCode::Home => 0,
                KeyCode::End => position.maximum(),
                _ => position.offset,
            }
        });
        if scrolled {
            InputUse::Scrolled
        } else {
            InputUse::Used
        }
    }
    pub fn input_handler(&self) -> TargetInputHandler {
        let scroll = self.clone();
        TargetInputHandler::new(move |event| scroll.handle_event(&event))
    }
    /// Scroll form bodies with pages, bounds and the wheel; arrows remain focus navigation.
    pub fn pages_input_handler(&self) -> TargetInputHandler {
        let scroll = self.clone();
        TargetInputHandler::new(move |event| {
            let accepts = match &event {
                TerminalEvent::Key(key) => matches!(
                    navigation_code(key),
                    Some(KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End)
                ),
                TerminalEvent::FullscreenMouse(_) => true,
                _ => false,
            };
            if accepts {
                scroll.handle_event(&event)
            } else {
                InputUse::Unused
            }
        })
    }
}

/// One translation owns the terminal's paging aliases for all scroll consumers.
pub(crate) fn navigation_code(key: &KeyEvent) -> Option<KeyCode> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Up, KeyModifiers::ALT) => Some(KeyCode::PageUp),
        (KeyCode::Down, KeyModifiers::ALT) => Some(KeyCode::PageDown),
        (KeyCode::Up, modifiers) if modifiers == KeyModifiers::ALT | KeyModifiers::SHIFT => {
            Some(KeyCode::Home)
        }
        (KeyCode::Down, modifiers) if modifiers == KeyModifiers::ALT | KeyModifiers::SHIFT => {
            Some(KeyCode::End)
        }
        (
            code @ (KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End),
            KeyModifiers::NONE,
        ) => Some(code),
        _ => None,
    }
}

#[cfg(test)]
#[path = "popup_pending_reveal_test.rs"]
mod pending_reveal_test;
