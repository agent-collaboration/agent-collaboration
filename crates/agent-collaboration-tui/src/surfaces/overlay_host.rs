//! Root siblings paint in stack order. Each measured surface owns a concrete View.
use crate::surfaces::{
    focus_model::TargetId,
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
        match overlay.layout.clone().resolve(context.stack.viewport()) {
            OverlayPlacement::Anchored {target,preferred_width,maximum_width,maximum_height}=>element!(AnchoredPanel(key:overlay.id.get(),context,parent:overlay.parent,target,preferred_width,maximum_width,maximum_height) {#(Some(overlay.view.render()))}).into_any(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnchorBounds {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
}
impl AnchorBounds {
    fn from_cells(cells: &[(u16, u16)]) -> Option<Self> {
        let left = cells.iter().map(|cell| cell.0).min()?;
        let top = cells.iter().map(|cell| cell.1).min()?;
        Some(Self {
            left,
            top,
            width: cells
                .iter()
                .map(|cell| cell.0)
                .max()?
                .saturating_sub(left)
                .saturating_add(1),
            height: cells
                .iter()
                .map(|cell| cell.1)
                .max()?
                .saturating_sub(top)
                .saturating_add(1),
        })
    }
}
struct AnchorPublication {
    parent: SurfaceContext,
    target: TargetId,
    bounds: State<Option<AnchorBounds>>,
}
impl Hook for AnchorPublication {
    fn poll_change(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        // InputOwner publishes after every child's draw. Observe that complete
        // map before requesting another layout; never infer geometry on resize.
        if let Some(cells) = self
            .parent
            .stack
            .hit_regions()
            .published_anchor_cells(self.parent.surface, &self.target)
        {
            let measured = AnchorBounds::from_cells(&cells);
            if self.bounds.get() != measured {
                self.bounds.set(measured);
                return std::task::Poll::Ready(());
            }
        }
        std::task::Poll::Pending
    }
}
#[derive(Props)]
struct AnchoredPanelProps<'a> {
    #[iocraft(required)]
    context: SurfaceContext,
    #[iocraft(required)]
    parent: SurfaceId,
    #[iocraft(required)]
    target: TargetId,
    preferred_width: u16,
    maximum_width: u16,
    maximum_height: u16,
    children: Vec<AnyElement<'a>>,
}
fn anchored_bounds(
    anchor: AnchorBounds,
    preferred_width: u16,
    maximum_width: u16,
    maximum_height: u16,
    viewport: (u16, u16),
) -> Option<AnchorBounds> {
    let width = preferred_width
        .max(anchor.width)
        .min(maximum_width)
        .min(viewport.0);
    let below_start = anchor.top.saturating_add(anchor.height).min(viewport.1);
    let below = viewport.1.saturating_sub(below_start);
    let above = anchor.top.min(viewport.1);
    let place_below = below >= maximum_height || (above < maximum_height && below >= above);
    let height = maximum_height.min(if place_below { below } else { above });
    if width == 0 || height == 0 {
        return None;
    }
    Some(AnchorBounds {
        left: anchor.left.min(viewport.0.saturating_sub(width)),
        top: if place_below {
            below_start
        } else {
            above.saturating_sub(height)
        },
        width,
        height,
    })
}
#[component]
fn AnchoredPanel<'a>(
    props: &mut AnchoredPanelProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let parent = SurfaceContext {
        stack: props.context.stack,
        surface: props.parent,
    };
    let bounds = hooks.use_state(|| {
        parent
            .stack
            .hit_regions()
            .published_anchor_cells(parent.surface, &props.target)
            .and_then(|cells| AnchorBounds::from_cells(&cells))
    });
    let observation = hooks.use_hook(|| AnchorPublication {
        parent,
        target: props.target.clone(),
        bounds,
    });
    observation.parent = parent;
    observation.target = props.target.clone();
    observation.bounds = bounds;
    let placement = bounds.get().and_then(|anchor| {
        anchored_bounds(
            anchor,
            props.preferred_width,
            props.maximum_width,
            props.maximum_height,
            parent.stack.viewport(),
        )
    });
    let children = std::mem::take(&mut props.children);
    match placement {
        Some(placement)=>element!(SurfacePanel(context:props.context,left:placement.left,top:placement.top,width:Size::Length(u32::from(placement.width)),height:Size::Length(u32::from(placement.height)),max_width:placement.width,max_height:placement.height,position:Position::Absolute,overlay:true,children)).into_any(),
        // A newly opened parent or Resize may not have published its field yet.
        None=>element!(View(width:0,height:0)).into_any(),
    }
}

#[cfg(test)]
#[path = "popup_anchor_test.rs"]
mod anchor_test;
