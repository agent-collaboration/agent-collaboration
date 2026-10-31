//! In-flow measurement, actual clipped hits, focus reveal and a visible scrollbar.
use super::{ScrollHandle, scroll_model::ScrollRevealTarget};
use crate::{
    actions::Availability,
    focus_model::TargetId,
    hit_regions::{HitRegionKind, use_target_geometry},
    surface_handle::SurfaceContext,
    theme::{TextRole, UiGlyph},
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
struct ViewportMeasure(ScrollHandle);
impl Hook for ViewportMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        self.0
            .measure_viewport(drawer.size().height, drawer.canvas_position().y);
    }
}
struct ContentMeasure(ScrollHandle);
impl Hook for ContentMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        self.0.measure_content(drawer.size().height);
    }
}
#[derive(Clone)]
struct ScrollContext {
    scroll: ScrollHandle,
    layout_offset: u16,
}
#[derive(Props)]
struct ScrollContentProps<'a> {
    #[iocraft(required)]
    scroll: ScrollHandle,
    offset: u16,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn ScrollContent<'a>(
    props: &mut ScrollContentProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    hooks.use_hook(|| ContentMeasure(props.scroll.clone())).0 = props.scroll.clone();
    let children = std::mem::take(&mut props.children);
    element!(View(position:Position::Relative,top:-i32::from(props.offset),width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
        ContextProvider(value:Context::owned(ScrollContext {scroll:props.scroll.clone(),layout_offset:props.offset})) {#(children)}
    })
}
#[derive(Props)]
pub struct ScrollListProps<'a> {
    #[iocraft(required)]
    pub id: TargetId,
    #[iocraft(required)]
    pub scroll: ScrollHandle,
    pub grow: bool,
    pub align_bottom: bool,
    pub keyboard_target: Option<TargetId>,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn ScrollList<'a>(
    props: &mut ScrollListProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = *hooks.use_context::<SurfaceContext>();
    use_target_geometry(
        &mut hooks,
        context,
        props.id.clone(),
        HitRegionKind::ScrollArea,
    );
    hooks.use_hook(|| ViewportMeasure(props.scroll.clone())).0 = props.scroll.clone();
    let position = props.scroll.position();
    let children = std::mem::take(&mut props.children);
    let keyboard_target = props.keyboard_target.as_ref().unwrap_or(&props.id);
    let availability = context
        .stack
        .snapshot()
        .surface(context.surface)
        .and_then(|surface| {
            surface
                .focus
                .targets()
                .iter()
                .find(|target| target.id == *keyboard_target)
        })
        .map_or(Availability::Available, |target| target.availability.get());
    let focused = availability.is_available() && context.is_focused(keyboard_target);
    let theme = context.theme();
    let visible = position.viewport_height;
    let thumb = if position.natural_height == 0 {
        0
    } else {
        u16::try_from(
            (u32::from(visible) * u32::from(visible) / u32::from(position.natural_height))
                .max(1)
                .min(u32::from(visible)),
        )
        .unwrap_or(visible)
    };
    let thumb_top = if position.maximum() == 0 {
        0
    } else {
        u16::try_from(
            u32::from(position.offset) * u32::from(visible.saturating_sub(thumb))
                / u32::from(position.maximum()),
        )
        .unwrap_or(0)
    };
    element!(View(width:100pct,min_height:0,flex_grow:if props.grow {1.0_f32}else{0.0_f32},flex_shrink:1.0_f32,padding_right:1,overflow:Overflow::Hidden,flex_direction:FlexDirection::Column) {
        #(props.align_bottom.then(||element!(View(flex_grow:1.0_f32,flex_shrink:1.0_f32,min_height:0))))
        ScrollContent(scroll:props.scroll.clone(),offset:position.offset,children)
        #((position.maximum()>0).then(||element! {View(position:Position::Absolute,right:0,top:0,width:1,height:visible,flex_direction:FlexDirection::Column) {
            #((0..visible).map(|row|element!(Text(content:if row>=thumb_top&&row<thumb_top+thumb {UiGlyph::ScrollThumb.text()}else{UiGlyph::ScrollTrack.text()},color:theme.foreground(if focused {TextRole::Focused}else{TextRole::Muted})))))
        }}))
        #(match availability {Availability::Available=>None,Availability::Unavailable {reason}=>Some(element!(Text(content:reason,color:theme.foreground(TextRole::Disabled))))})
    })
}
struct RevealHook {
    context: Option<ScrollContext>,
    focused: bool,
    target_id: Option<TargetId>,
    last_target: Option<ScrollRevealTarget>,
}
impl Hook for RevealHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let Some(context) = self.context.as_ref().filter(|_| self.focused) else {
            if let Some(previous) = self.last_target.take()
                && let Some(context) = self.context.as_ref()
            {
                context.scroll.forget_reveal(previous);
            }
            return;
        };
        let position = context.scroll.position();
        let logical_top = i32::from(drawer.canvas_position().y) - i32::from(position.viewport_top)
            + i32::from(context.layout_offset);
        let target = ScrollRevealTarget {
            target_id: self.target_id.clone(),
            logical_top,
            height: drawer.size().height,
            viewport_height: position.viewport_height,
        };
        self.last_target = Some(target.clone());
        context.scroll.reveal(target);
    }
}
pub(crate) fn use_scroll_reveal(
    hooks: &mut Hooks<'_, '_>,
    focused: bool,
    target_id: Option<TargetId>,
) {
    let context = hooks
        .try_use_context::<ScrollContext>()
        .map(|context| context.clone());
    let hook = hooks.use_hook(|| RevealHook {
        context: context.clone(),
        focused,
        target_id: target_id.clone(),
        last_target: None,
    });
    hook.context = context;
    hook.focused = focused;
    hook.target_id = target_id;
}
#[derive(Props)]
pub struct ScrollRevealProps<'a> {
    pub focused: bool,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn ScrollReveal<'a>(
    props: &mut ScrollRevealProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let target_id = hooks
        .try_use_context::<SurfaceContext>()
        .and_then(|context| {
            context
                .stack
                .snapshot()
                .surface(context.surface)
                .and_then(|surface| surface.focus.current().cloned())
        });
    use_scroll_reveal(&mut hooks, props.focused, target_id);
    match props.children.first_mut() {
        Some(child) => child.into(),
        None => element!(View).into_any(),
    }
}

#[derive(Props)]
pub(crate) struct TargetScrollRevealProps<'a> {
    #[iocraft(required)]
    pub(crate) target: TargetId,
    pub(crate) focused: bool,
    pub(crate) children: Vec<AnyElement<'a>>,
}
#[component]
pub(crate) fn TargetScrollReveal<'a>(
    props: &mut TargetScrollRevealProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    use_scroll_reveal(&mut hooks, props.focused, Some(props.target.clone()));
    match props.children.first_mut() {
        Some(child) => child.into(),
        None => element!(View).into_any(),
    }
}
