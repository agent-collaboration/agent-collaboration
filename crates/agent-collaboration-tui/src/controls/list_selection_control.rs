//! A list is one keyboard target; keyed pointer rows share its scoped selection.
use super::{ListRowKey, ListSelectionError, ListSelectionState, ScrollHandle};
use crate::{
    actions::ActionId,
    focus_model::{FocusError, InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceContext,
    surface_stack::{SurfaceId, SurfaceModel, SurfaceStack},
};
use iocraft::prelude::*;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Eq, PartialEq)]
enum SelectionReveal {
    FollowSelection,
    PreserveViewport,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ListNavigationPolicy {
    #[default]
    RowPaging,
    ContentScroll,
}

#[derive(Clone)]
pub struct ListSelectionBinding {
    pub parent: SurfaceContext,
    pub id: TargetId,
    pub value: State<ListSelectionState>,
    pub on_select: Handler<Option<ListRowKey>>,
    pub activation: ActionId,
    /// The consumer's layout budget in rows, evaluated at the routed event.
    pub page_rows: Arc<dyn Fn() -> usize + Send + Sync>,
}
#[derive(Debug)]
pub enum ListControlError {
    Selection(ListSelectionError),
    Focus(FocusError),
}
impl std::fmt::Display for ListControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "cannot update list: {self:?}")
    }
}
impl std::error::Error for ListControlError {}

#[derive(Clone)]
pub struct ListSelectionControl {
    binding: ListSelectionBinding,
    scroll: ScrollHandle,
    navigation_policy: ListNavigationPolicy,
    hover_selection: bool,
    selection_reveal: Arc<Mutex<SelectionReveal>>,
}
impl ListSelectionControl {
    pub fn new(binding: ListSelectionBinding) -> Self {
        Self {
            scroll: ScrollHandle::new(binding.parent.stack),
            binding,
            navigation_policy: ListNavigationPolicy::default(),
            hover_selection: false,
            selection_reveal: Arc::new(Mutex::new(SelectionReveal::FollowSelection)),
        }
    }
    pub fn with_navigation_policy(mut self, policy: ListNavigationPolicy) -> Self {
        self.navigation_policy = policy;
        self
    }
    /// Opt in to selection/focus on pointer motion, preserving the viewport.
    pub fn with_hover_selection(mut self, enabled: bool) -> Self {
        self.hover_selection = enabled;
        self
    }
    pub(crate) fn reveals_selection(&self) -> bool {
        *self
            .selection_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            == SelectionReveal::FollowSelection
    }
    fn set_selection_reveal(&self, reveal: SelectionReveal) {
        *self
            .selection_reveal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = reveal;
    }
    pub fn id(&self) -> &TargetId {
        &self.binding.id
    }
    pub fn scroll(&self) -> ScrollHandle {
        self.scroll.clone()
    }
    pub fn scroll_target(&self) -> TargetId {
        self.binding.id.child("scroll")
    }
    pub fn row_target(&self, key: &ListRowKey) -> TargetId {
        self.binding.id.child(&format!("row:{}", key.as_str()))
    }
    pub fn selected(&self) -> Option<ListRowKey> {
        self.binding.value.read().selected().cloned()
    }
    pub fn rows(&self) -> Vec<ListRowKey> {
        self.binding.value.read().rows().to_vec()
    }
    /// Install these targets in the parent model before its first event.
    pub fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = vec![
            TargetSpec::new(self.binding.id.clone())
                .with_input(self.input_handler())
                .with_activation(self.binding.activation.clone()),
        ];
        targets.extend(self.row_targets(self.binding.value.read().rows()));
        targets.push(
            TargetSpec::new(self.scroll_target())
                .pointer_only()
                .with_input(self.input_handler()),
        );
        targets
    }
    /// Update only this control's row targets; retain the other parent controls.
    pub fn replace_rows(&self, rows: Vec<ListRowKey>) -> Result<bool, ListControlError> {
        let snapshot = self.binding.parent.stack.snapshot();
        let model = base_model(&snapshot, self.binding.parent.surface);
        if !model.is_some_and(|model| {
            model
                .focus
                .targets()
                .iter()
                .any(|target| target.id == self.binding.id)
        }) {
            return Ok(false);
        }
        let previous = self.binding.value.read().clone();
        let mut next = previous.clone();
        next.replace_rows(rows)
            .map_err(ListControlError::Selection)?;
        if next == previous {
            return Ok(true);
        }
        let previous_targets = previous
            .rows()
            .iter()
            .map(|key| self.row_target(key))
            .collect::<HashSet<_>>();
        let new_targets = self.row_targets(next.rows());
        let replaced = self.binding.parent.stack.update(|stack| {
            let Some(model) = base_model_mut(stack, self.binding.parent.surface) else {
                return Ok(false);
            };
            if !model
                .focus
                .targets()
                .iter()
                .any(|target| target.id == self.binding.id)
            {
                return Ok(false);
            }
            let mut targets = Vec::new();
            for target in model.focus.targets() {
                if !previous_targets.contains(&target.id) {
                    targets.push(target.clone());
                    if target.id == self.binding.id {
                        targets.extend(new_targets.iter().cloned());
                    }
                }
            }
            model
                .focus
                .replace_targets(targets)
                .map_err(ListControlError::Focus)?;
            Ok(true)
        })?;
        if replaced {
            let changed = previous.selected() != next.selected();
            let selected = next.selected().cloned();
            let mut value = self.binding.value;
            value.set(next);
            self.binding.parent.stack.hit_regions().invalidate();
            if changed {
                self.set_selection_reveal(SelectionReveal::FollowSelection);
                (self.binding.on_select)(selected);
            }
        }
        Ok(replaced)
    }
    fn input_handler(&self) -> TargetInputHandler {
        let control = self.clone();
        TargetInputHandler::new(move |event| {
            if control.binding.parent.stack.snapshot().top_id() != control.binding.parent.surface {
                return InputUse::Unused;
            }
            let (code, wheel) = match &event {
                TerminalEvent::Key(key)
                    if key.kind != KeyEventKind::Release && key.modifiers.is_empty() =>
                {
                    match key.code {
                        KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::PageUp
                        | KeyCode::PageDown
                        | KeyCode::Home
                        | KeyCode::End => (key.code, false),
                        _ => return InputUse::Unused,
                    }
                }
                TerminalEvent::FullscreenMouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => (KeyCode::Up, true),
                    MouseEventKind::ScrollDown => (KeyCode::Down, true),
                    _ => return InputUse::Unused,
                },
                _ => return InputUse::Unused,
            };
            if control.navigation_policy == ListNavigationPolicy::ContentScroll
                && (wheel
                    || matches!(
                        code,
                        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
                    ))
            {
                return control.scroll.handle_event(&event);
            }
            let mut next = control.binding.value.read().clone();
            let changed = next.navigate(code, (control.binding.page_rows)());
            if changed {
                control.publish_selection(next, SelectionReveal::FollowSelection);
            }
            if wheel && changed {
                InputUse::Scrolled
            } else {
                InputUse::Used
            }
        })
    }
    fn publish_selection(&self, next: ListSelectionState, reveal: SelectionReveal) {
        self.set_selection_reveal(reveal);
        let selected = next.selected().cloned();
        let mut value = self.binding.value;
        value.set(next);
        (self.binding.on_select)(selected);
    }
    fn row_targets(&self, rows: &[ListRowKey]) -> Vec<TargetSpec> {
        let mut targets = Vec::with_capacity(rows.len());
        for key in rows {
            let target = self.row_target(key);
            let key = key.clone();
            let control = self.clone();
            let input = TargetInputHandler::new(move |event| {
                let reveal = match event {
                    TerminalEvent::FullscreenMouse(mouse) => match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => SelectionReveal::FollowSelection,
                        MouseEventKind::Moved if control.hover_selection => {
                            SelectionReveal::PreserveViewport
                        }
                        _ => return InputUse::Unused,
                    },
                    _ => return InputUse::Unused,
                };
                let parent = control.binding.parent;
                if parent.stack.snapshot().top_id() != parent.surface {
                    return InputUse::Unused;
                }
                let mut next = control.binding.value.read().clone();
                if !next.rows().contains(&key) {
                    return InputUse::Used;
                }
                control.set_selection_reveal(reveal);
                parent.stack.focus(parent.surface, &control.binding.id);
                if next.select(&key) {
                    control.publish_selection(next, reveal);
                }
                InputUse::Used
            });
            targets.push(TargetSpec::new(target).pointer_only().with_input(input));
        }
        targets
    }
}
fn base_model(stack: &SurfaceStack, surface: SurfaceId) -> Option<&SurfaceModel> {
    match surface {
        SurfaceId::Screen => stack.surface(surface),
        SurfaceId::Overlay(id) => stack.overlay_base_model(id),
    }
}
fn base_model_mut(stack: &mut SurfaceStack, surface: SurfaceId) -> Option<&mut SurfaceModel> {
    match surface {
        SurfaceId::Screen => stack.surface_mut(surface),
        SurfaceId::Overlay(id) => stack.overlay_base_model_mut(id),
    }
}
