//! A field's model updates and pointer targets share the lifetime of its parent scope.
use super::combo_box::ComboDropdown;
use super::{
    ComboBoxState, ComboTransition, DuplicateOptionKey, OptionKey, ScrollHandle, SelectOption,
};
use crate::{
    actions::{ActionRegistry, ScopeId},
    focus_model::{FocusModel, InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceContext,
    surface_stack::{
        OverlayId, OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use unicode_width::UnicodeWidthStr;

const MINIMUM_DROPDOWN_WIDTH: usize = 12;
const MAXIMUM_DROPDOWN_HEIGHT: u16 = 9;

#[derive(Debug)]
pub enum ComboModelError {
    Identity(crate::actions::IdentifierError),
    Focus(crate::focus_model::FocusError),
    Actions(crate::actions::ActionError),
    Options(DuplicateOptionKey),
    Overlay(crate::surface_stack::OverlayIdentityExhausted),
    ParentUnavailable,
}
impl std::fmt::Display for ComboModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid dropdown model: {self:?}")
    }
}
impl std::error::Error for ComboModelError {}

#[derive(Clone)]
pub struct ComboBoxBinding {
    pub parent: SurfaceContext,
    pub field: TargetId,
    /// The existing parent body scroll, when this field lives in a ScrollList.
    pub parent_scroll: Option<ScrollHandle>,
    pub value: State<ComboBoxState>,
    pub on_pick: Handler<OptionKey>,
}

#[derive(Clone)]
pub struct ComboBoxControl {
    binding: ComboBoxBinding,
    scroll: ScrollHandle,
}
impl ComboBoxControl {
    pub fn new(binding: ComboBoxBinding) -> Self {
        Self {
            scroll: ScrollHandle::new(binding.parent.stack),
            binding,
        }
    }

    /// Open against the bound field; the host owns fit, flipping and resize.
    pub fn open(&self) -> Result<OverlayId, ComboModelError> {
        let binding = &self.binding;
        if binding.parent.stack.snapshot().top_id() != binding.parent.surface {
            return Err(ComboModelError::ParentUnavailable);
        }
        self.assert_parent_scroll_binding();
        if let Some(scroll) = &binding.parent_scroll {
            scroll.reveal_measured(&binding.field);
        }
        let mut next = binding.value.read().clone();
        next.begin();
        let layout = self.dropdown_layout(&next);
        let model = self.build_model(next.options())?;
        let mut value = binding.value;
        value.set(next);
        self.scroll.reset();
        let field = binding.field.clone();
        let scroll = self.scroll.clone();
        binding
            .parent
            .stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dropdown {
                    field: field.clone(),
                },
                model,
                view: OverlayView::new(move || {
                    element!(ComboDropdown(field:field.clone(),value,scroll:scroll.clone()))
                        .into_any()
                }),
                layout,
            })
            .map_err(ComboModelError::Overlay)
    }

    /// A late provider update cannot mutate a draft whose parent has already closed.
    pub fn replace_options(&self, options: Vec<SelectOption>) -> Result<bool, ComboModelError> {
        let binding = &self.binding;
        let snapshot = binding.parent.stack.snapshot();
        if snapshot.surface(binding.parent.surface).is_none() {
            return Ok(false);
        }
        let mut next = binding.value.read().clone();
        next.replace_options(options)
            .map_err(ComboModelError::Options)?;
        let targets = self.targets(next.options());
        if let Some(overlay) = snapshot.dropdown_for(binding.parent.surface, &binding.field) {
            binding
                .parent
                .stack
                .replace_targets_if_open(SurfaceId::Overlay(overlay.id), targets)
                .map_err(ComboModelError::Focus)?;
            binding.parent.stack.hit_regions().invalidate();
        }
        self.refresh_layout(&next);
        let mut value = binding.value;
        value.set(next);
        Ok(true)
    }

    fn dropdown_layout(&self, state: &ComboBoxState) -> OverlayLayout {
        let option_width = state
            .matching()
            .into_iter()
            .map(|option| {
                let label_width = option.label.width()
                    + option
                        .secondary
                        .as_deref()
                        .map_or(0, |text| text.width() + 1)
                    + 5;
                let reason_width = match &option.availability {
                    crate::actions::Availability::Available => 0,
                    crate::actions::Availability::Unavailable { reason } => reason.width() + 5,
                };
                label_width.max(reason_width)
            })
            .max()
            .unwrap_or("No match".width() + 3);
        // Filter prefix, gap, cursor and border are outside the option rows.
        let preferred_width = option_width
            .max(state.query().text().width() + 11)
            .max(MINIMUM_DROPDOWN_WIDTH);
        OverlayLayout::Anchored {
            target: self.binding.field.clone(),
            preferred_width: u16::try_from(preferred_width).unwrap_or(u16::MAX),
            maximum_width: u16::MAX,
            maximum_height: MAXIMUM_DROPDOWN_HEIGHT,
        }
    }

    fn refresh_layout(&self, state: &ComboBoxState) {
        let binding = &self.binding;
        let snapshot = binding.parent.stack.snapshot();
        let Some(overlay) = snapshot.dropdown_for(binding.parent.surface, &binding.field) else {
            return;
        };
        let layout = self.dropdown_layout(state);
        if overlay.layout != layout {
            binding.parent.stack.hit_regions().invalidate();
            binding
                .parent
                .stack
                .update(|stack| stack.set_overlay_layout_if_open(overlay.id, layout));
        }
    }

    fn assert_parent_scroll_binding(&self) {
        #[cfg(any(debug_assertions, test))]
        if self.binding.parent_scroll.is_none()
            && let Some(map) = self.binding.parent.stack.hit_regions().published()
            && map
                .surfaces
                .iter()
                .any(|region| region.surface == self.binding.parent.surface)
        {
            assert!(
                map.targets
                    .iter()
                    .any(|region| region.surface == self.binding.parent.surface
                        && region.target == self.binding.field
                        && !region.cells.is_empty()),
                "{} requires parent_scroll to reveal its offscreen field before opening",
                self.binding.field.as_str()
            );
        }
    }

    fn build_model(&self, options: &[SelectOption]) -> Result<SurfaceModel, ComboModelError> {
        Ok(SurfaceModel {
            focus: FocusModel::new(self.targets(options)).map_err(ComboModelError::Focus)?,
            actions: ActionRegistry::new(
                ScopeId::new("dropdown").map_err(ComboModelError::Identity)?,
                self.help_actions(),
            )
            .map_err(ComboModelError::Actions)?,
            primary_action: None,
        })
    }

    fn help_actions(&self) -> Vec<crate::actions::ActionDef> {
        let snapshot = self.binding.parent.stack.snapshot();
        let event = KeyEvent::new(KeyEventKind::Press, KeyCode::F(1));
        snapshot
            .surface(self.binding.parent.surface)
            .and_then(|surface| {
                surface
                    .actions
                    .definitions()
                    .iter()
                    .find(|action| action.keys.iter().any(|key| key.matches(&event)))
                    .cloned()
            })
            .into_iter()
            .collect()
    }
    fn targets(&self, options: &[SelectOption]) -> Vec<TargetSpec> {
        let binding = self.binding.clone();
        let input_binding = binding.clone();
        let input_control = self.clone();
        let keyboard_scroll = self.scroll.clone();
        let input = TargetInputHandler::new(move |event| {
            if matches!(&event,TerminalEvent::Key(key) if matches!(super::scroll_model::navigation_code(key),Some(KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End)))
            {
                return keyboard_scroll.handle_event(&event);
            }

            let mut value = input_binding.value;
            let mut next = value.read().clone();
            let transition = next.handle_event(&event);
            if transition == ComboTransition::Unused {
                return InputUse::Unused;
            }
            input_control.refresh_layout(&next);
            value.set(next);
            if let ComboTransition::Picked(key) = transition {
                input_binding.parent.stack.close_top();
                (input_binding.on_pick)(key);
            }
            InputUse::Used
        });
        let mut targets = vec![TargetSpec::new(binding.field.child("filter")).with_input(input)];
        for option in options {
            let key = option.key.clone();
            let binding = binding.clone();
            targets.push(
                TargetSpec::new(binding.field.child(&format!("option:{}", key.as_str())))
                    .pointer_only()
                    .with_input(TargetInputHandler::new(move |event| {
                        let TerminalEvent::FullscreenMouse(mouse) = event else {
                            return InputUse::Unused;
                        };
                        if !matches!(
                            mouse.kind,
                            MouseEventKind::Moved | MouseEventKind::Down(MouseButton::Left)
                        ) {
                            return InputUse::Unused;
                        }
                        let mut value = binding.value;
                        let mut next = value.read().clone();
                        if !next.matching().iter().any(|option| option.key == key) {
                            return InputUse::Used;
                        }
                        next.hover(&key);
                        let picked = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                            .then(|| next.pick())
                            .flatten();
                        value.set(next);
                        if let Some(picked) = picked {
                            binding.parent.stack.close_top();
                            (binding.on_pick)(picked);
                        }
                        InputUse::Used
                    })),
            );
        }
        targets.push(
            TargetSpec::new(binding.field.child("scroll"))
                .pointer_only()
                .with_input(self.scroll.input_handler()),
        );
        targets
    }
}
