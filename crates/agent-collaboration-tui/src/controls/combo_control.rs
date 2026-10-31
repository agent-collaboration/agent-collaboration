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

    pub fn open(&self, layout: OverlayLayout) -> Result<OverlayId, ComboModelError> {
        let binding = &self.binding;
        if binding.parent.stack.snapshot().top_id() != binding.parent.surface {
            return Err(ComboModelError::ParentUnavailable);
        }
        let mut next = binding.value.read().clone();
        next.begin();
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
        let mut value = binding.value;
        value.set(next);
        Ok(true)
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
