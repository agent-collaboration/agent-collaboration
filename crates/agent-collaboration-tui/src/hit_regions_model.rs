//! Derived draw data; no component, consumer callback or reactive state lives here.
use crate::{focus_model::TargetId, surface_stack::SurfaceId};
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HitRegionKind {
    #[default]
    Target,
    ScrollArea,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetRegion {
    pub surface: SurfaceId,
    pub target: TargetId,
    pub kind: HitRegionKind,
    pub cells: Vec<(u16, u16)>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceRegion {
    pub surface: SurfaceId,
    pub cells: Vec<(u16, u16)>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HitMapSnapshot {
    pub frame: u64,
    pub targets: Vec<TargetRegion>,
    pub surfaces: Vec<SurfaceRegion>,
}
impl HitMapSnapshot {
    pub fn surface_contains(&self, surface: SurfaceId, point: (u16, u16)) -> Option<bool> {
        self.surfaces
            .iter()
            .find(|region| region.surface == surface)
            .map(|region| region.cells.contains(&point))
    }
    pub fn target_at(
        &self,
        surface: SurfaceId,
        point: (u16, u16),
        kind: HitRegionKind,
    ) -> Option<&TargetId> {
        self.targets
            .iter()
            .rev()
            .find(|region| {
                region.surface == surface && region.kind == kind && region.cells.contains(&point)
            })
            .map(|region| &region.target)
    }
}
#[derive(Default)]
pub(crate) struct HitRegionCache {
    staged: HitMapSnapshot,
    published: Option<HitMapSnapshot>,
    frames: u64,
    invalidation: u64,
    staged_invalidation: u64,
}
pub struct HitRegions(Ref<HitRegionCache>);
impl Copy for HitRegions {}
impl Clone for HitRegions {
    fn clone(&self) -> Self {
        *self
    }
}
impl HitRegions {
    pub(crate) fn new(hooks: &mut Hooks<'_, '_>) -> Self {
        Self(hooks.use_ref_default())
    }
    pub fn published(self) -> Option<HitMapSnapshot> {
        self.0.read().published.clone()
    }
    pub fn invalidate(mut self) {
        let mut cache = self.0.write();
        cache.invalidation = cache.invalidation.wrapping_add(1);
        cache.published = None;
    }
    pub(crate) fn begin(mut self) {
        let mut cache = self.0.write();
        cache.staged = HitMapSnapshot::default();
        cache.staged_invalidation = cache.invalidation;
    }
    pub(crate) fn stage_target(mut self, region: TargetRegion) {
        self.0.write().staged.targets.push(region);
    }
    pub(crate) fn stage_surface(mut self, region: SurfaceRegion) {
        self.0.write().staged.surfaces.push(region);
    }
    pub(crate) fn publish(mut self) {
        let mut cache = self.0.write();
        cache.frames = cache.frames.saturating_add(1);
        let mut frame = std::mem::take(&mut cache.staged);
        frame.frame = cache.frames;
        cache.published = (cache.staged_invalidation == cache.invalidation).then_some(frame);
    }
}
