//! Reactive ownership of the pure stack; callbacks never run under its borrow.
use crate::{
    focus_model::{FocusError, TargetId, TargetSpec},
    hit_regions_model::HitRegions,
    surface_stack::{
        OverlayId, OverlayIdentityExhausted, OverlaySpec, SurfaceId, SurfaceModel, SurfaceStack,
    },
    theme::Theme,
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
