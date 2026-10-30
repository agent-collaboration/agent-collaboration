//! Help projects the source scope; the stack owns suspended dialog models.
use super::{HelpContent, ScrollHandle, button_target};
use crate::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    focus_model::{FocusModel, InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceStackHandle,
    surface_stack::{
        OverlayId, OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
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
    WithinDialog(OverlayId),
    OwnDialog(OverlayId),
}
impl HelpPresentation {
    fn surface(&self) -> SurfaceId {
        match self {
            Self::WithinDialog(id) | Self::OwnDialog(id) => SurfaceId::Overlay(*id),
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
    pub fn show(&self, content: HelpContent) -> Result<bool, HelpError> {
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
        let model = self.model(scroll.clone())?;
        let presentation = if stack
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
            title: "Back".into(),
            keys: vec!["F1".parse().map_err(HelpError::Keys)?],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: self.close_handler(),
        };
        let close = ActionDef {
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
