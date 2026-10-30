//! Root siblings paint in stack order. Each measured surface owns a concrete View.
use crate::{
    hit_regions::stage_surface, hit_regions_model::HitRegions, surface_handle::SurfaceContext,
    surface_stack::SurfaceId, theme::SurfaceRole,
};
use iocraft::{ComponentDrawer, Hook, prelude::*};

struct SurfacePaintHook {
    regions: HitRegions,
    surface: SurfaceId,
    background: Color,
    overlay: bool,
}
impl Hook for SurfacePaintHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        stage_surface(self.regions, self.surface, drawer);
        let size = drawer.size();
        let gutter = self.overlay && drawer.canvas_position().x > 0;
        let left = if gutter { -1 } else { 0 };
        let width = usize::from(size.width) + usize::from(gutter);
        let mut canvas = drawer.canvas();
        canvas.clear_text(left, 0, width, usize::from(size.height));
        canvas.set_background_color(left, 0, width, usize::from(size.height), self.background);
    }
}
#[derive(Props)]
struct SurfacePanelProps<'a> {
    #[iocraft(required)]
    context: SurfaceContext,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    overlay: bool,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn SurfacePanel<'a>(
    props: &mut SurfacePanelProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = props.context;
    let background = context.theme().background(if props.overlay {
        SurfaceRole::Raised
    } else {
        SurfaceRole::Base
    });
    let hook = hooks.use_hook(|| SurfacePaintHook {
        regions: context.stack.hit_regions(),
        surface: context.surface,
        background,
        overlay: props.overlay,
    });
    hook.regions = context.stack.hit_regions();
    hook.surface = context.surface;
    hook.background = background;
    hook.overlay = props.overlay;
    let children = std::mem::take(&mut props.children);
    element! {
        View(position:Position::Absolute,left:props.left,top:props.top,width:props.width,height:props.height,overflow:Overflow::Hidden,
            background_color:background,flex_direction:FlexDirection::Column) {
            ContextProvider(value:Context::owned(context)) { #(children) }
        }
    }
}
#[derive(Props)]
pub struct OverlayHostProps<'a> {
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn OverlayHost<'a>(
    props: &mut OverlayHostProps<'a>,
    hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let stack = context.stack.snapshot();
    let (width, height) = context.stack.viewport();
    let children = std::mem::take(&mut props.children);
    let overlays=stack.overlays().iter().map(|overlay| {
        let (left,top,width,height)=overlay.layout.resolve(context.stack.viewport());
        element!(SurfacePanel(key:overlay.id.get(),context:SurfaceContext{stack:context.stack,surface:SurfaceId::Overlay(overlay.id)},
            left,top,width,height,overlay:true) { #(Some(overlay.view.render())) }).into_any()
    }).collect::<Vec<_>>();
    element! {
        View(width,height,position:Position::Relative) {
            SurfacePanel(context:SurfaceContext {stack:context.stack,surface:SurfaceId::Screen},width,height) { #(children) }
            #(overlays)
        }
    }
}
