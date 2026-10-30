//! Root siblings paint in stack order. Each measured surface owns a concrete View.
use crate::{
    hit_regions::stage_surface,
    hit_regions_model::HitRegions,
    surface_handle::SurfaceContext,
    surface_stack::{OverlayPlacement, SurfaceId},
};
use iocraft::{ComponentDrawer, Hook, prelude::*};

/// The host owns the resolved bounds; dialog chrome uses the same sizing axes.
#[derive(Clone, Copy)]
pub(crate) struct SurfaceSizing {
    pub width: Size,
    pub height: Size,
    pub max_width: u16,
    pub max_height: u16,
}

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
    width: Size,
    height: Size,
    max_width: u16,
    max_height: u16,
    position: Position,
    overlay: bool,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn SurfacePanel<'a>(
    props: &mut SurfacePanelProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = props.context;
    let background = context.theme().background(context.surface_role());
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
    let sizing = SurfaceSizing {
        width: props.width,
        height: props.height,
        max_width: props.max_width,
        max_height: props.max_height,
    };
    element! {
        View(position:props.position,left:props.left,top:props.top,width:props.width,height:props.height,max_width:props.max_width,max_height:props.max_height,overflow:Overflow::Hidden,
            background_color:background,flex_direction:FlexDirection::Column) {
            ContextProvider(value:Context::owned(context)) {
                ContextProvider(value:Context::owned(sizing)) { #(children) }
            }
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
        let context=SurfaceContext {stack:context.stack,surface:SurfaceId::Overlay(overlay.id)};
        match overlay.layout.resolve(context.stack.viewport()) {
            OverlayPlacement::Fixed {left,top,width,height}=>element!(SurfacePanel(key:overlay.id.get(),context,left,top,width:Size::Length(u32::from(width)),height:Size::Length(u32::from(height)),max_width:width,max_height:height,position:Position::Absolute,overlay:true) {#(Some(overlay.view.render()))}).into_any(),
            OverlayPlacement::Centered {width:panel_width,height:panel_height,max_width,max_height,margins}=>element!(View(key:overlay.id.get(),position:Position::Absolute,left:0,top:0,width,height,padding_left:margins.left,padding_right:margins.right,padding_top:margins.top,padding_bottom:margins.bottom,align_items:AlignItems::Center,justify_content:JustifyContent::Center) {SurfacePanel(context,width:panel_width,height:panel_height,max_width,max_height,position:Position::Relative,overlay:true) {#(Some(overlay.view.render()))}}).into_any(),
        }
    }).collect::<Vec<_>>();
    element! {
        View(width,height,position:Position::Relative) {
            SurfacePanel(context:SurfaceContext {stack:context.stack,surface:SurfaceId::Screen},width:Size::Length(u32::from(width)),height:Size::Length(u32::from(height)),max_width:width,max_height:height,position:Position::Absolute) { #(children) }
            #(overlays)
        }
    }
}
