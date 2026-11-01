//! One in-memory stack; a model exists before its first draw.
use crate::{
    actions::{ActionId, ActionRegistry},
    surfaces::focus_model::{FocusModel, TargetId},
};
use iocraft::prelude::*;
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct OverlayView(Arc<dyn Fn() -> AnyElement<'static> + Send + Sync>);
impl OverlayView {
    pub fn new(render: impl Fn() -> AnyElement<'static> + Send + Sync + 'static) -> Self {
        Self(Arc::new(render))
    }
    pub fn render(&self) -> AnyElement<'static> {
        (self.0)()
    }
}
impl Default for OverlayView {
    fn default() -> Self {
        Self::new(|| element!(View).into_any())
    }
}
impl fmt::Debug for OverlayView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OverlayView")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OverlayId(u64);
impl OverlayId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SurfaceId {
    Screen,
    Overlay(OverlayId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OverlayKind {
    Menu,
    Dropdown { field: TargetId },
    Dialog,
}

#[derive(Clone, Debug)]
pub struct SurfaceModel {
    pub focus: FocusModel,
    pub actions: ActionRegistry,
    pub primary_action: Option<ActionId>,
}

impl SurfaceModel {
    pub(crate) fn reconcile_focus(&mut self) -> bool {
        let actions = &self.actions;
        let primary_action = self.primary_action.as_ref();
        self.focus.reconcile_with_priority(|target| {
            target.activation.as_ref().is_some_and(|action| {
                primary_action == Some(action)
                    || actions.definitions().iter().any(|definition| {
                        definition.id == *action
                            && definition.risk == crate::actions::ActionRisk::Destructive
                    })
            })
        })
    }

    fn mount_initial_focus(&mut self) {
        self.focus.capture_initial_focus();
        self.reconcile_focus();
    }
}

#[derive(Clone, Debug)]
pub struct OverlaySpec {
    pub kind: OverlayKind,
    pub model: SurfaceModel,
    pub view: OverlayView,
    pub layout: OverlayLayout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OverlayLayout {
    Anchored {
        target: TargetId,
        preferred_width: u16,
        maximum_width: u16,
        maximum_height: u16,
    },
    Dialog {
        presentation: crate::DialogPresentation,
        margins: crate::DialogMargins,
    },
    CenteredCells {
        width: u16,
        height: u16,
    },
    AtCells {
        left: u16,
        top: u16,
        width: u16,
        height: u16,
    },
}
impl Default for OverlayLayout {
    fn default() -> Self {
        Self::CenteredCells {
            width: 40,
            height: 12,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayPlacement {
    Anchored {
        target: TargetId,
        preferred_width: u16,
        maximum_width: u16,
        maximum_height: u16,
    },
    Fixed {
        left: u16,
        top: u16,
        width: u16,
        height: u16,
    },
    Centered {
        width: Size,
        height: Size,
        max_width: u16,
        max_height: u16,
        margins: crate::DialogMargins,
    },
}
impl OverlayLayout {
    pub fn resolve(self, viewport: (u16, u16)) -> OverlayPlacement {
        match self {
            Self::Anchored {
                target,
                preferred_width,
                maximum_width,
                maximum_height,
            } => OverlayPlacement::Anchored {
                target,
                preferred_width,
                maximum_width,
                maximum_height,
            },
            Self::Dialog {
                presentation,
                margins,
            } => {
                let left = margins.left.min(viewport.0);
                let right = margins.right.min(viewport.0.saturating_sub(left));
                let top = margins.top.min(viewport.1);
                let bottom = margins.bottom.min(viewport.1.saturating_sub(top));
                let ((width, max_width), (height, max_height)) = presentation.resolve(
                    viewport.0.saturating_sub(left + right),
                    viewport.1.saturating_sub(top + bottom),
                );
                OverlayPlacement::Centered {
                    width,
                    height,
                    max_width,
                    max_height,
                    margins: crate::DialogMargins {
                        left,
                        right,
                        top,
                        bottom,
                    },
                }
            }
            Self::CenteredCells { width, height } => {
                let width = width.min(viewport.0);
                let height = height.min(viewport.1);
                OverlayPlacement::Fixed {
                    left: (viewport.0 - width) / 2,
                    top: (viewport.1 - height) / 2,
                    width,
                    height,
                }
            }
            Self::AtCells {
                left,
                top,
                width,
                height,
            } => OverlayPlacement::Fixed {
                left,
                top,
                width: width.min(viewport.0.saturating_sub(left)),
                height: height.min(viewport.1.saturating_sub(top)),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub id: OverlayId,
    pub parent: SurfaceId,
    pub kind: OverlayKind,
    pub model: SurfaceModel,
    pub restore_focus: Option<TargetId>,
    pub view: OverlayView,
    pub layout: OverlayLayout,
    suspended_model: Option<SurfaceModel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayIdentityExhausted;
impl fmt::Display for OverlayIdentityExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("overlay identity exhausted")
    }
}
impl std::error::Error for OverlayIdentityExhausted {}

#[derive(Clone, Debug)]
pub struct SurfaceStack {
    screen: SurfaceModel,
    overlays: Vec<Overlay>,
    suspended_dropdown: Option<Overlay>,
    next_identity: u64,
}
impl SurfaceStack {
    pub(crate) fn empty() -> Self {
        Self::new(SurfaceModel {
            focus: FocusModel::empty(),
            actions: ActionRegistry::empty(),
            primary_action: None,
        })
    }
    pub fn new(mut screen: SurfaceModel) -> Self {
        screen.mount_initial_focus();
        Self {
            screen,
            overlays: Vec::new(),
            suspended_dropdown: None,
            next_identity: 1,
        }
    }
    pub fn top_id(&self) -> SurfaceId {
        self.overlays
            .last()
            .map_or(SurfaceId::Screen, |overlay| SurfaceId::Overlay(overlay.id))
    }
    pub fn top(&self) -> &SurfaceModel {
        self.overlays
            .last()
            .map_or(&self.screen, |overlay| &overlay.model)
    }
    pub fn top_mut(&mut self) -> &mut SurfaceModel {
        self.overlays
            .last_mut()
            .map_or(&mut self.screen, |overlay| &mut overlay.model)
    }
    pub fn overlays(&self) -> &[Overlay] {
        &self.overlays
    }
    pub(crate) fn set_overlay_layout_if_open(&mut self, id: OverlayId, layout: OverlayLayout) {
        if let Some(overlay) = self
            .overlays
            .iter_mut()
            .chain(self.suspended_dropdown.iter_mut())
            .find(|overlay| overlay.id == id)
        {
            overlay.layout = layout;
        }
    }
    pub fn surface(&self, id: SurfaceId) -> Option<&SurfaceModel> {
        match id {
            SurfaceId::Screen => Some(&self.screen),
            SurfaceId::Overlay(id) => self
                .overlays
                .iter()
                .chain(self.suspended_dropdown.iter())
                .find(|overlay| overlay.id == id)
                .map(|overlay| &overlay.model),
        }
    }
    pub fn surface_mut(&mut self, id: SurfaceId) -> Option<&mut SurfaceModel> {
        match id {
            SurfaceId::Screen => Some(&mut self.screen),
            SurfaceId::Overlay(id) => self
                .overlays
                .iter_mut()
                .chain(self.suspended_dropdown.iter_mut())
                .find(|overlay| overlay.id == id)
                .map(|overlay| &mut overlay.model),
        }
    }
    pub(crate) fn overlay_base_model(&self, id: OverlayId) -> Option<&SurfaceModel> {
        self.overlays
            .iter()
            .chain(self.suspended_dropdown.iter())
            .find(|overlay| overlay.id == id)
            .map(|overlay| overlay.suspended_model.as_ref().unwrap_or(&overlay.model))
    }
    pub(crate) fn overlay_base_model_mut(&mut self, id: OverlayId) -> Option<&mut SurfaceModel> {
        self.overlays
            .iter_mut()
            .chain(self.suspended_dropdown.iter_mut())
            .find(|overlay| overlay.id == id)
            .map(|overlay| {
                overlay
                    .suspended_model
                    .as_mut()
                    .unwrap_or(&mut overlay.model)
            })
    }
    pub(crate) fn suspend_dialog_model(&mut self, id: OverlayId, help: SurfaceModel) -> bool {
        let Some(overlay) = self.overlays.iter_mut().find(|overlay| {
            overlay.id == id
                && overlay.kind == OverlayKind::Dialog
                && overlay.suspended_model.is_none()
        }) else {
            return false;
        };
        overlay.suspended_model = Some(std::mem::replace(&mut overlay.model, help));
        true
    }
    pub(crate) fn restore_dialog_model(&mut self, id: OverlayId) -> bool {
        let active = self.top_id() == SurfaceId::Overlay(id);
        let Some(overlay) = self.overlays.iter_mut().find(|overlay| overlay.id == id) else {
            return false;
        };
        let Some(model) = overlay.suspended_model.take() else {
            return false;
        };
        overlay.model = model;
        if active {
            overlay.model.reconcile_focus();
        }
        true
    }
    pub(crate) fn dropdown_for(&self, parent: SurfaceId, field: &TargetId) -> Option<&Overlay> {
        self.overlays
            .iter()
            .chain(self.suspended_dropdown.iter())
            .find(|overlay| {
                overlay.parent == parent
                    && overlay.kind
                        == OverlayKind::Dropdown {
                            field: field.clone(),
                        }
            })
    }
    pub(crate) fn suspend_dropdown(&mut self, id: OverlayId) -> bool {
        if self.suspended_dropdown.is_some()
            || !self.overlays.last().is_some_and(|overlay| {
                overlay.id == id && matches!(overlay.kind, OverlayKind::Dropdown { .. })
            })
        {
            return false;
        }
        self.suspended_dropdown = self.overlays.pop();
        true
    }
    pub(crate) fn restore_dropdown(&mut self) -> bool {
        let Some(dropdown) = self.suspended_dropdown.take() else {
            return false;
        };
        if self.top_id() != dropdown.parent {
            self.suspended_dropdown = Some(dropdown);
            return false;
        }
        let mut dropdown = dropdown;
        dropdown.model.reconcile_focus();
        self.overlays.push(dropdown);
        true
    }
    pub fn is_covered(&self, id: SurfaceId) -> bool {
        self.surface(id).is_some() && self.top_id() != id
    }
    pub fn open_overlay(
        &mut self,
        mut spec: OverlaySpec,
    ) -> Result<OverlayId, OverlayIdentityExhausted> {
        spec.model.mount_initial_focus();
        let next = self
            .next_identity
            .checked_add(1)
            .ok_or(OverlayIdentityExhausted)?;
        let id = OverlayId(self.next_identity);
        let restore_focus = self.top().focus.current().cloned();
        self.overlays.push(Overlay {
            id,
            parent: self.top_id(),
            kind: spec.kind,
            model: spec.model,
            restore_focus,
            view: spec.view,
            layout: spec.layout,
            suspended_model: None,
        });
        self.next_identity = next;
        Ok(id)
    }
    pub fn close_top(&mut self) -> Option<OverlayId> {
        let closed = self.overlays.pop()?;
        if self
            .suspended_dropdown
            .as_ref()
            .is_some_and(|dropdown| dropdown.parent == SurfaceId::Overlay(closed.id))
        {
            self.suspended_dropdown = None;
        }
        self.top_mut().focus.restore(closed.restore_focus.as_ref());
        self.top_mut().reconcile_focus();
        Some(closed.id)
    }
}

#[cfg(test)]
#[path = "surface_stack_test.rs"]
mod tests;
