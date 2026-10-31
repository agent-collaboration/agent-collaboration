//! Help borrows a scope's definitions and restores a dialog's entire input model.
use super::{ScrollHandle, button_target};
use crate::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    focus_model::{FocusModel, InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceStackHandle,
    surface_stack::{
        OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum HelpError {
    SourceUnavailable,
    Keys(crate::actions::KeyChordError),
    Identity(crate::actions::IdentifierError),
    Actions(crate::actions::ActionError),
    Focus(crate::focus_model::FocusError),
    Overlay(crate::surface_stack::OverlayIdentityExhausted),
}
impl std::fmt::Display for HelpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "cannot open Help: {self:?}")
    }
}
impl std::error::Error for HelpError {}

#[derive(Clone)]
enum HelpPresentation {
    WithinDialog {
        surface: SurfaceId,
        saved: SurfaceModel,
    },
    OwnDialog {
        surface: SurfaceId,
    },
}
impl HelpPresentation {
    fn surface(&self) -> SurfaceId {
        match self {
            Self::WithinDialog { surface, .. } | Self::OwnDialog { surface } => *surface,
        }
    }
}
#[derive(Clone)]
struct HelpSession {
    presentation: HelpPresentation,
    source: ActionRegistry,
    scroll: ScrollHandle,
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
        self.session(surface).map(|session| session.source)
    }
    pub fn scroll(&self, surface: SurfaceId) -> Option<ScrollHandle> {
        self.session(surface).map(|session| session.scroll)
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
    /// The source registry may be saved inside this control, so its opening handler is weak.
    pub fn open_handler(&self, on_error: Handler<HelpError>) -> Handler<()> {
        let weak = Arc::downgrade(&self.0);
        Handler::from(move |()| {
            if let Some(state) = weak.upgrade()
                && let Err(error) = Self(state).show()
            {
                on_error(error);
            }
        })
    }
    pub fn show(&self) -> Result<bool, HelpError> {
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
        let source = stack.top().actions.clone();
        let scroll = ScrollHandle::new(self.0.stack);
        let model = self.model(scroll.clone())?;
        let presentation = if stack
            .overlays()
            .last()
            .is_some_and(|overlay| overlay.kind == OverlayKind::Dialog)
        {
            let surface = stack.top_id();
            let saved = stack.top().clone();
            self.0.stack.update(|stack| *stack.top_mut() = model);
            HelpPresentation::WithinDialog { surface, saved }
        } else if stack.top_id() == SurfaceId::Screen {
            let control = self.clone();
            let id = self
                .0
                .stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Dialog,
                    model,
                    view: OverlayView::new(move || {
                        element!(super::help_dialog::HelpFrame(control:control.clone())).into_any()
                    }),
                    layout: OverlayLayout::Dialog {
                        presentation: crate::DialogPresentation::default(),
                        margins: crate::DialogMargins::default(),
                    },
                })
                .map_err(HelpError::Overlay)?;
            HelpPresentation::OwnDialog {
                surface: SurfaceId::Overlay(id),
            }
        } else {
            return Err(HelpError::SourceUnavailable);
        };
        *self
            .0
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(HelpSession {
            presentation,
            source,
            scroll,
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
            HelpPresentation::WithinDialog { surface, saved } => {
                if let Some(model) = stack.surface_mut(surface) {
                    *model = saved;
                    true
                } else {
                    false
                }
            }
            HelpPresentation::OwnDialog { surface } => {
                if stack.top_id() == surface {
                    stack.close_top();
                    true
                } else {
                    false
                }
            }
        });
        self.0.stack.hit_regions().invalidate();
        closed
    }
    fn close_handler(&self) -> Handler<()> {
        let weak = Arc::downgrade(&self.0);
        Handler::from(move |()| {
            if let Some(state) = weak.upgrade() {
                Self(state).close();
            }
        })
    }
    fn model(&self, scroll: ScrollHandle) -> Result<SurfaceModel, HelpError> {
        let close_id = ActionId::new(format!("{}:close-help", self.0.action.as_str()))
            .map_err(HelpError::Identity)?;
        let back = ActionDef {
            id: self.0.action.clone(),
            title: "Back",
            keys: vec!["F1".parse().map_err(HelpError::Keys)?],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: self.close_handler(),
        };
        let close = ActionDef {
            id: close_id.clone(),
            title: "Close Help",
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
