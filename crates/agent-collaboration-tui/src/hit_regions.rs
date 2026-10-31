//! Geometry comes from the current draw and its real inherited clip.
pub use crate::hit_regions_model::{HitMapSnapshot, HitRegionKind, HitRegions};
use crate::{
    focus_model::TargetId,
    hit_regions_model::{SurfaceRegion, TargetRegion},
    surface_handle::SurfaceContext,
    surface_stack::SurfaceId,
};
use iocraft::{ComponentDrawer, Hook, prelude::*};

pub(crate) fn visible_cells(drawer: &mut ComponentDrawer<'_>) -> Vec<(u16, u16)> {
    let origin = drawer.canvas_position();
    let size = drawer.size();
    let canvas = drawer.canvas();
    let mut cells = Vec::new();
    for row in 0..size.height {
        for column in 0..size.width {
            let (Ok(local_column), Ok(local_row)) = (isize::try_from(column), isize::try_from(row))
            else {
                continue;
            };
            if canvas.cell(local_column, local_row).is_none() {
                continue;
            }
            if let (Ok(column), Ok(row)) = (
                u16::try_from(i32::from(origin.x) + i32::from(column)),
                u16::try_from(i32::from(origin.y) + i32::from(row)),
            ) {
                cells.push((column, row));
            }
        }
    }
    cells
}
struct TargetGeometryHook {
    regions: HitRegions,
    surface: SurfaceId,
    target: TargetId,
    kind: HitRegionKind,
}
impl Hook for TargetGeometryHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        self.regions.stage_target(TargetRegion {
            surface: self.surface,
            target: self.target.clone(),
            kind: self.kind,
            cells: visible_cells(drawer),
        });
    }
}
pub(crate) fn use_target_geometry(
    hooks: &mut Hooks<'_, '_>,
    context: SurfaceContext,
    target: TargetId,
    kind: HitRegionKind,
) {
    let hook = hooks.use_hook(|| TargetGeometryHook {
        regions: context.stack.hit_regions(),
        surface: context.surface,
        target: target.clone(),
        kind,
    });
    hook.regions = context.stack.hit_regions();
    hook.surface = context.surface;
    hook.target = target;
    hook.kind = kind;
}
#[derive(Props)]
pub struct HitRegionProps<'a> {
    #[iocraft(required)]
    pub target: TargetId,
    pub kind: HitRegionKind,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn HitRegion<'a>(
    props: &mut HitRegionProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(&mut hooks, context, props.target.clone(), props.kind);
    match props.children.first_mut() {
        Some(child) => child.into(),
        None => element!(View).into_any(),
    }
}
struct FramePublicationHook(HitRegions);
impl Hook for FramePublicationHook {
    fn pre_component_draw(&mut self, _drawer: &mut ComponentDrawer) {
        self.0.begin();
    }
    fn post_component_draw(&mut self, _drawer: &mut ComponentDrawer) {
        self.0.publish();
    }
}
pub(crate) fn use_frame_publication(hooks: &mut Hooks<'_, '_>, regions: HitRegions) {
    hooks.use_hook(|| FramePublicationHook(regions)).0 = regions;
}
pub(crate) fn stage_surface(regions: HitRegions, surface: SurfaceId, drawer: &mut ComponentDrawer) {
    regions.stage_surface(SurfaceRegion {
        surface,
        cells: visible_cells(drawer),
    });
}
