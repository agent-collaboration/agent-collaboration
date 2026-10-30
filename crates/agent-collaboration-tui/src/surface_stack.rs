//! One in-memory stack; a model exists before its first draw.
use crate::{
    actions::{ActionId, ActionRegistry},
    focus_model::{FocusModel, TargetId},
};
use std::fmt;

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

#[derive(Clone, Debug)]
pub struct OverlaySpec {
    pub kind: OverlayKind,
    pub model: SurfaceModel,
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub id: OverlayId,
    pub parent: SurfaceId,
    pub kind: OverlayKind,
    pub model: SurfaceModel,
    pub restore_focus: Option<TargetId>,
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
    next_identity: u64,
}
impl SurfaceStack {
    pub fn new(screen: SurfaceModel) -> Self {
        Self {
            screen,
            overlays: Vec::new(),
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
    pub fn surface(&self, id: SurfaceId) -> Option<&SurfaceModel> {
        match id {
            SurfaceId::Screen => Some(&self.screen),
            SurfaceId::Overlay(id) => self
                .overlays
                .iter()
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
                .find(|overlay| overlay.id == id)
                .map(|overlay| &mut overlay.model),
        }
    }
    pub fn is_covered(&self, id: SurfaceId) -> bool {
        self.surface(id).is_some() && self.top_id() != id
    }
    pub fn open_overlay(
        &mut self,
        spec: OverlaySpec,
    ) -> Result<OverlayId, OverlayIdentityExhausted> {
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
        });
        self.next_identity = next;
        Ok(id)
    }
    pub fn close_top(&mut self) -> Option<OverlayId> {
        let closed = self.overlays.pop()?;
        self.top_mut().focus.restore(closed.restore_focus.as_ref());
        Some(closed.id)
    }
}
