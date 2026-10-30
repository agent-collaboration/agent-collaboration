//! Reactive ownership of the pure stack; callbacks never run under its borrow.
use crate::{
    focus_model::{FocusError, TargetId, TargetSpec},
    hit_regions_model::HitRegions,
    surface_stack::{
        OverlayId, OverlayIdentityExhausted, OverlayKind, OverlaySpec, SurfaceId, SurfaceModel,
        SurfaceStack,
    },
    theme::{SurfaceRole, Theme},
};
use iocraft::prelude::*;

pub struct SurfaceStackHandle {
    stack: State<SurfaceStack>,
    regions: HitRegions,
    viewport: State<(u16, u16)>,
    hovered: State<Option<(SurfaceId, TargetId)>>,
}
impl Copy for SurfaceStackHandle {}
impl Clone for SurfaceStackHandle {
    fn clone(&self) -> Self {
        *self
    }
}
impl SurfaceStackHandle {
    pub fn snapshot(self) -> SurfaceStack {
        self.stack.read().clone()
    }
    pub fn hit_regions(self) -> HitRegions {
        self.regions
    }
    pub fn viewport(self) -> (u16, u16) {
        self.viewport.get()
    }
    pub(crate) fn set_viewport(mut self, width: u16, height: u16) {
        self.regions.invalidate();
        self.viewport.set((width, height));
    }
    pub fn hovered(self) -> Option<(SurfaceId, TargetId)> {
        self.hovered.read().clone()
    }
    pub(crate) fn set_hovered(mut self, target: Option<(SurfaceId, TargetId)>) {
        if *self.hovered.read() != target {
            self.hovered.set(target);
        }
    }
    pub(crate) fn update<TResult>(
        mut self,
        update: impl FnOnce(&mut SurfaceStack) -> TResult,
    ) -> TResult {
        let mut next = self.snapshot();
        let result = update(&mut next);
        self.stack.set(next);
        result
    }
    pub fn open_overlay(self, spec: OverlaySpec) -> Result<OverlayId, OverlayIdentityExhausted> {
        self.update(|stack| stack.open_overlay(spec))
    }
    pub fn close_top(self) -> Option<OverlayId> {
        self.update(SurfaceStack::close_top)
    }
    /// Update the continuing model without remounting its view, scroll or draft state.
    /// While Help replaces a dialog, the update applies to its suspended base model.
    /// Focus follows the prior TargetId, falling back only when it is no longer enabled.
    pub fn update_overlay_if_open(
        mut self,
        overlay: OverlayId,
        update: impl FnOnce(&mut SurfaceModel),
    ) -> bool {
        let mut next = self.snapshot();
        let Some(model) = next.overlay_base_model_mut(overlay) else {
            return false;
        };
        let focused = model.focus.current().cloned();
        update(model);
        model.focus.restore(focused.as_ref());
        self.regions.invalidate();
        self.stack.set(next);
        true
    }
    pub fn focus(self, surface: SurfaceId, target: &TargetId) -> bool {
        self.update(|stack| {
            stack
                .surface_mut(surface)
                .is_some_and(|surface| surface.focus.focus(target))
        })
    }
    pub fn replace_targets_if_open(
        self,
        surface: SurfaceId,
        targets: Vec<TargetSpec>,
    ) -> Result<bool, FocusError> {
        self.update(|stack| match stack.surface_mut(surface) {
            Some(surface) => surface.focus.replace_targets(targets).map(|()| true),
            None => Ok(false),
        })
    }
}

pub fn use_surface_stack(
    hooks: &mut Hooks<'_, '_>,
    build: impl FnOnce(SurfaceStackHandle) -> SurfaceModel,
) -> SurfaceStackHandle {
    let stack = hooks.use_state(SurfaceStack::empty);
    let regions = HitRegions::new(hooks);
    let viewport = hooks.use_state(|| (0, 0));
    let hovered = hooks.use_state(|| None);
    let mut initialized = hooks.use_ref(|| false);
    let handle = SurfaceStackHandle {
        stack,
        regions,
        viewport,
        hovered,
    };
    if !initialized.get() {
        let model = build(handle);
        handle.update(|stack| *stack = SurfaceStack::new(model));
        initialized.set(true);
    }
    handle
}

#[derive(Clone, Copy)]
pub struct SurfaceContext {
    pub stack: SurfaceStackHandle,
    pub surface: SurfaceId,
}
impl SurfaceContext {
    pub fn surface_role(self) -> SurfaceRole {
        let stack = self.stack.snapshot();
        if self.surface == SurfaceId::Screen
            || stack.overlays().iter().any(|overlay| {
                SurfaceId::Overlay(overlay.id) == self.surface
                    && overlay.kind == OverlayKind::Dialog
            })
        {
            SurfaceRole::Base
        } else {
            SurfaceRole::Raised
        }
    }
    pub fn theme(self) -> Theme {
        if self.stack.snapshot().is_covered(self.surface) {
            Theme::covered()
        } else {
            Theme::default()
        }
    }
    pub fn is_focused(self, target: &TargetId) -> bool {
        let stack = self.stack.snapshot();
        stack.top_id() == self.surface && stack.top().focus.current() == Some(target)
    }
    pub fn is_hovered(self, target: &TargetId) -> bool {
        self.stack
            .hovered()
            .as_ref()
            .is_some_and(|(surface, id)| *surface == self.surface && id == target)
    }
}
