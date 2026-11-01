//! Reactive ownership of the pure stack; callbacks never run under its borrow.
use crate::{
    foundation::theme::{SurfaceRole, Theme},
    surfaces::{
        focus_model::{FocusError, TargetId, TargetSpec},
        hit_regions_model::HitRegions,
        surface_stack::{
            OverlayId, OverlayIdentityExhausted, OverlayKind, OverlaySpec, SurfaceId, SurfaceModel,
            SurfaceStack,
        },
    },
};
use iocraft::prelude::*;
use std::sync::Arc;

struct SurfaceStackSlot {
    stack: SurfaceStack,
    write_token: Arc<()>,
}
impl SurfaceStackSlot {
    fn new(stack: SurfaceStack) -> Self {
        Self {
            stack,
            write_token: Arc::new(()),
        }
    }
}

pub struct SurfaceStackHandle {
    slot: State<SurfaceStackSlot>,
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
        self.slot.read().stack.clone()
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
        self,
        update: impl FnOnce(&mut SurfaceStack) -> TResult,
    ) -> TResult {
        let mut next = self.snapshot();
        let result = update(&mut next);
        self.publish_stack(next, None);
        result
    }
    /// The only interaction-model publication point. A retained token also
    /// protects a conditional commit from intervening writes during preparation.
    fn publish_stack(mut self, next_stack: SurfaceStack, expected_token: Option<&Arc<()>>) -> bool {
        let mut current_slot = self.slot.write();
        if expected_token.is_some_and(|token| !Arc::ptr_eq(token, &current_slot.write_token)) {
            return false;
        }
        let previous_slot =
            std::mem::replace(&mut *current_slot, SurfaceStackSlot::new(next_stack));
        drop(current_slot);
        drop(previous_slot);
        true
    }
    pub fn open_overlay(self, spec: OverlaySpec) -> Result<OverlayId, OverlayIdentityExhausted> {
        self.update(|stack| stack.open_overlay(spec))
    }
    pub fn close_top(self) -> Option<OverlayId> {
        self.update(SurfaceStack::close_top)
    }
    /// Update the continuing model without remounting its view, scroll or draft state.
    /// While Help replaces a dialog, the update applies to its suspended base model.
    /// Current and displaced focus intent survive replacement; only the active
    /// surface reconciles eligibility while covered models keep their intent.
    /// The updater runs once with no stack borrow held. Any interaction-stack
    /// publication during it, including a no-op reentrant write, takes precedence:
    /// this returns false and leaves that live state in place. A closed target
    /// also returns false, without invoking the updater.
    pub fn update_overlay_if_open(
        self,
        overlay: OverlayId,
        update: impl FnOnce(&mut SurfaceModel),
    ) -> bool {
        let (mut candidate, observed_token) = {
            let current_slot = self.slot.read();
            let Some(model) = current_slot.stack.overlay_base_model(overlay) else {
                return false;
            };
            (model.clone(), Arc::clone(&current_slot.write_token))
        };
        let previous_focus = candidate.focus.clone();
        update(&mut candidate);
        let mut next = {
            let current_slot = self.slot.read();
            if !Arc::ptr_eq(&observed_token, &current_slot.write_token) {
                return false;
            }
            current_slot.stack.clone()
        };
        candidate.focus.inherit_focus_intent(&previous_focus);
        let Some(model) = next.overlay_base_model_mut(overlay) else {
            return false;
        };
        *model = candidate;
        next.top_mut().reconcile_focus();
        if self.publish_stack(next, Some(&observed_token)) {
            self.regions.invalidate();
            true
        } else {
            false
        }
    }
    pub(crate) fn reconcile_active_focus(self) {
        let mut next = self.snapshot();
        if next.top_mut().reconcile_focus() {
            self.publish_stack(next, None);
        }
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
            Some(surface) => surface.focus.replace_targets(targets).map(|()| {
                surface.reconcile_focus();
                true
            }),
            None => Ok(false),
        })
    }
}

pub fn use_surface_stack(
    hooks: &mut Hooks<'_, '_>,
    build: impl FnOnce(SurfaceStackHandle) -> SurfaceModel,
) -> SurfaceStackHandle {
    let slot = hooks.use_state(|| SurfaceStackSlot::new(SurfaceStack::empty()));
    let regions = HitRegions::new(hooks);
    let viewport = hooks.use_state(|| (0, 0));
    let hovered = hooks.use_state(|| None);
    let mut initialized = hooks.use_ref(|| false);
    let handle = SurfaceStackHandle {
        slot,
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

#[cfg(test)]
#[path = "surface_handle_reentry_test.rs"]
mod reentry_test;
