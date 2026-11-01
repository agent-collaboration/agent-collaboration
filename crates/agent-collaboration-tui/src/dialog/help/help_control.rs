//! Help projects the source scope; the stack owns suspended dialog models.
use crate::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    surfaces::{
        focus_model::{FocusModel, InputUse, TargetId, TargetInputHandler, TargetSpec},
        surface_handle::SurfaceStackHandle,
        surface_stack::{
            OverlayId, OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId,
            SurfaceModel,
        },
    },
};
use crate::{
    controls::{fields::button_target, list::ScrollHandle},
    dialog::help::{HelpContent, HelpInputConvention},
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum HelpError {
    SourceUnavailable,
    Keys(crate::foundation::KeyChordError),
    Identity(crate::actions::IdentifierError),
    Actions(crate::actions::ActionError),
    Focus(crate::surfaces::focus_model::FocusError),
    Overlay(crate::surfaces::surface_stack::OverlayIdentityExhausted),
}
impl std::fmt::Display for HelpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "cannot open Help: {self:?}")
    }
}
impl std::error::Error for HelpError {}

#[derive(Clone)]
enum HelpPresentation {
    WithinDialog(OverlayId),
    OwnDialog(OverlayId),
    DropdownInDialog {
        dialog: OverlayId,
        dropdown: OverlayId,
    },
    DropdownOnScreen {
        dialog: OverlayId,
        dropdown: OverlayId,
    },
}
impl HelpPresentation {
    fn surface(&self) -> SurfaceId {
        match self {
            Self::WithinDialog(id) | Self::OwnDialog(id) => SurfaceId::Overlay(*id),
            Self::DropdownInDialog { dialog, .. } | Self::DropdownOnScreen { dialog, .. } => {
                SurfaceId::Overlay(*dialog)
            }
        }
    }
}
#[derive(Clone)]
struct HelpSession {
    presentation: HelpPresentation,
    scroll: ScrollHandle,
    content: HelpContent,
}
struct HelpState {
    stack: SurfaceStackHandle,
    action: ActionId,
    session: Mutex<Option<HelpSession>>,
}
#[derive(Clone)]
pub struct HelpControl(Arc<HelpState>);
impl HelpControl {
    pub fn new(stack: SurfaceStackHandle, action: ActionId) -> Self {
        Self(Arc::new(HelpState {
            stack,
            action,
            session: Mutex::new(None),
        }))
    }
    pub fn body_target(&self) -> TargetId {
        TargetId::for_action(&self.0.action).child("help-body")
    }
    pub fn scroll_target(&self) -> TargetId {
        TargetId::for_action(&self.0.action).child("help-scroll")
    }
    pub fn action(&self) -> &ActionId {
        &self.0.action
    }
    pub fn source(&self, surface: SurfaceId) -> Option<ActionRegistry> {
        let session = self.session(surface)?;
        let stack = self.0.stack.snapshot();
        match session.presentation {
            HelpPresentation::WithinDialog(id) => stack.overlay_base_model(id),
            HelpPresentation::OwnDialog(_) => stack.surface(SurfaceId::Screen),
            HelpPresentation::DropdownInDialog { dropdown, .. }
            | HelpPresentation::DropdownOnScreen { dropdown, .. } => {
                stack.surface(SurfaceId::Overlay(dropdown))
            }
        }
        .map(|model| model.actions.clone())
    }
    pub fn scroll(&self, surface: SurfaceId) -> Option<ScrollHandle> {
        self.session(surface).map(|session| session.scroll)
    }
    pub fn content(&self, surface: SurfaceId) -> Option<HelpContent> {
        self.session(surface).map(|session| session.content)
    }
    pub fn is_active(&self, surface: SurfaceId) -> bool {
        self.session(surface).is_some()
    }
    fn session(&self, surface: SurfaceId) -> Option<HelpSession> {
        self.0
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|session| session.presentation.surface() == surface)
            .cloned()
    }
    /// Registries retain this handler, so it must not retain the control in return.
    pub fn open_handler(
        &self,
        content: impl Fn() -> HelpContent + Send + Sync + 'static,
        on_error: Handler<HelpError>,
    ) -> Handler<()> {
        let weak = Arc::downgrade(&self.0);
        Handler::from(move |()| {
            if let Some(state) = weak.upgrade()
                && let Err(error) = Self(state).show(content())
            {
                on_error(error);
            }
        })
    }
    pub fn show(&self, mut content: HelpContent) -> Result<bool, HelpError> {
        let stack = self.0.stack.snapshot();
        {
            let mut session = self
                .0
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if session
                .as_ref()
                .is_some_and(|session| stack.surface(session.presentation.surface()).is_some())
            {
                return Ok(false);
            }
            *session = None;
        }
        let scroll = ScrollHandle::new(self.0.stack);
        let opening_keys = stack
            .top()
            .actions
            .definitions()
            .iter()
            .find(|action| action.id == self.0.action)
            .map_or_else(Vec::new, |action| action.keys.clone());
        let model = self.model(scroll.clone(), opening_keys)?;
        let presentation = if let Some(dropdown) = stack
            .overlays()
            .last()
            .filter(|overlay| matches!(overlay.kind, OverlayKind::Dropdown { .. }))
        {
            let source = dropdown.id;
            let parent = dropdown.parent;
            if let SurfaceId::Overlay(id) = parent
                && !stack
                    .overlays()
                    .iter()
                    .any(|overlay| overlay.id == id && overlay.kind == OverlayKind::Dialog)
            {
                return Err(HelpError::SourceUnavailable);
            }
            if !self.0.stack.update(|stack| stack.suspend_dropdown(source)) {
                return Err(HelpError::SourceUnavailable);
            }
            let opened = match parent {
                SurfaceId::Overlay(dialog) => {
                    if self
                        .0
                        .stack
                        .update(|stack| stack.suspend_dialog_model(dialog, model))
                    {
                        Ok(HelpPresentation::DropdownInDialog {
                            dialog,
                            dropdown: source,
                        })
                    } else {
                        Err(HelpError::SourceUnavailable)
                    }
                }
                SurfaceId::Screen => {
                    self.open_help_dialog(model)
                        .map(|dialog| HelpPresentation::DropdownOnScreen {
                            dialog,
                            dropdown: source,
                        })
                }
            };
            let presentation = match opened {
                Ok(presentation) => presentation,
                Err(error) => {
                    self.0.stack.update(|stack| stack.restore_dropdown());
                    return Err(error);
                }
            };
            content.conventions = vec![
                HelpInputConvention {
                    key: "↑/↓".into(),
                    description: "Move highlight".into(),
                },
                HelpInputConvention {
                    key: "Typing".into(),
                    description: "Filter options".into(),
                },
                HelpInputConvention {
                    key: "Enter".into(),
                    description: "Choose option".into(),
                },
                HelpInputConvention {
                    key: "Esc".into(),
                    description: "Close dropdown".into(),
                },
            ];
            content
                .conventions
                .extend(HelpContent::default().conventions);
            presentation
        } else if stack
            .overlays()
            .last()
            .is_some_and(|overlay| overlay.kind == OverlayKind::Dialog)
        {
            let surface = stack.top_id();
            let SurfaceId::Overlay(id) = surface else {
                return Err(HelpError::SourceUnavailable);
            };
            if !self
                .0
                .stack
                .update(|stack| stack.suspend_dialog_model(id, model))
            {
                return Err(HelpError::SourceUnavailable);
            }
            HelpPresentation::WithinDialog(id)
        } else if stack.top_id() == SurfaceId::Screen {
            let id = self.open_help_dialog(model)?;
            HelpPresentation::OwnDialog(id)
        } else {
            return Err(HelpError::SourceUnavailable);
        };
        *self
            .0
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(HelpSession {
            presentation,
            scroll,
            content,
        });
        self.0.stack.hit_regions().invalidate();
        Ok(true)
    }
    pub fn close(&self) -> bool {
        let session = self
            .0
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(session) = session else {
            return false;
        };
        let closed = self.0.stack.update(|stack| match session.presentation {
            HelpPresentation::WithinDialog(id) => stack.restore_dialog_model(id),
            HelpPresentation::OwnDialog(id) => {
                if stack.top_id() == SurfaceId::Overlay(id) {
                    stack.close_top();
                    true
                } else {
                    false
                }
            }
            HelpPresentation::DropdownInDialog { dialog, .. } => {
                stack.restore_dialog_model(dialog) && stack.restore_dropdown()
            }
            HelpPresentation::DropdownOnScreen { dialog, .. } => {
                if stack.top_id() != SurfaceId::Overlay(dialog) {
                    false
                } else {
                    stack.close_top();
                    stack.restore_dropdown()
                }
            }
        });
        self.0.stack.hit_regions().invalidate();
        closed
    }
    fn open_help_dialog(&self, model: SurfaceModel) -> Result<OverlayId, HelpError> {
        let control = self.clone();
        self.0
            .stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model,
                view: OverlayView::new(move || {
                    element!(crate::dialog::help::help_dialog::HelpFrame(control:control.clone()))
                        .into_any()
                }),
                layout: OverlayLayout::Dialog {
                    presentation: crate::DialogPresentation::default(),
                    margins: crate::DialogMargins::default(),
                },
            })
            .map_err(HelpError::Overlay)
    }
    fn close_handler(&self) -> Handler<()> {
        let weak = Arc::downgrade(&self.0);
        Handler::from(move |()| {
            if let Some(state) = weak.upgrade() {
                Self(state).close();
            }
        })
    }
    fn model(
        &self,
        scroll: ScrollHandle,
        opening_keys: Vec<crate::foundation::KeyChord>,
    ) -> Result<SurfaceModel, HelpError> {
        let close_id = ActionId::new(format!("{}:close-help", self.0.action.as_str()))
            .map_err(HelpError::Identity)?;
        let back = ActionDef {
            toolbar_label: None,
            id: self.0.action.clone(),
            title: "Back".into(),
            keys: opening_keys,
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: self.close_handler(),
        };
        let close = ActionDef {
            toolbar_label: None,
            id: close_id.clone(),
            title: "Close Help".into(),
            keys: vec![
                "Esc".parse().map_err(HelpError::Keys)?,
                "Ctrl+Enter".parse().map_err(HelpError::Keys)?,
            ],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: self.close_handler(),
        };
        let close_input = self.close_handler();
        let keyboard_scroll = scroll.clone();
        let input = TargetInputHandler::new(move |event| {
            if matches!(&event,TerminalEvent::Key(key) if key.kind==KeyEventKind::Press&&key.code==KeyCode::Esc)
            {
                close_input(());
                InputUse::Used
            } else {
                keyboard_scroll.handle_event(&event)
            }
        });
        let targets = vec![
            TargetSpec::new(self.body_target()).with_input(input.clone()),
            button_target(TargetId::for_action(&back.id), &back).with_input(input.clone()),
            button_target(TargetId::for_action(&close.id), &close).with_input(input),
            TargetSpec::new(self.scroll_target())
                .pointer_only()
                .with_input(scroll.input_handler()),
        ];
        Ok(SurfaceModel {
            focus: FocusModel::new(targets).map_err(HelpError::Focus)?,
            actions: ActionRegistry::new(
                ScopeId::new("help").map_err(HelpError::Identity)?,
                vec![back, close],
            )
            .map_err(HelpError::Actions)?,
            primary_action: Some(close_id),
        })
    }
}

#[cfg(test)]
#[path = "dropdown_help_test.rs"]
mod dropdown_test;
#[cfg(test)]
#[path = "help_lifetime_test.rs"]
mod lifetime_test;
#[cfg(test)]
#[path = "help_control_test.rs"]
mod tests;
