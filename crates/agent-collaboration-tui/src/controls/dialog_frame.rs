//! Dialog chrome measures content axes within OverlayHost's resolved bounds.
use super::dialog_footer::DialogFooter;
use super::{Button, ScrollHandle, ScrollList};
use crate::{
    actions::Placement,
    focus_model::TargetId,
    overlay_host::SurfaceSizing,
    surface_handle::SurfaceContext,
    theme::{SurfaceRole, TextRole, Theme},
};
use iocraft::{ComponentDrawer, Hook, prelude::*};
struct FrameMeasure(State<u16>);
impl Hook for FrameMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.0.get() != drawer.size().height {
            self.0.set(drawer.size().height);
        }
    }
}
#[derive(Props)]
pub struct DialogFrameProps<'a> {
    pub title: String,
    #[iocraft(required)]
    pub scroll: ScrollHandle,
    #[iocraft(required)]
    pub scroll_target: TargetId,
    pub scroll_focus: Option<TargetId>,
    pub padding: Option<u16>,
    pub border_style: Option<BorderStyle>,
    pub border_color: Option<Color>,
    pub help_action: Option<AnyElement<'a>>,
    pub context: Vec<AnyElement<'a>>,
    pub children: Vec<AnyElement<'a>>,
    pub actions: Vec<AnyElement<'a>>,
    pub action_hints: Vec<AnyElement<'a>>,
}
#[component]
pub fn DialogFrame<'a>(
    props: &mut DialogFrameProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let context = hooks
        .try_use_context::<SurfaceContext>()
        .map(|context| *context);
    let theme = context.map_or(Theme::default(), SurfaceContext::theme);
    let sizing = hooks
        .try_use_context::<SurfaceSizing>()
        .map(|sizing| *sizing);
    let content_height = sizing.is_some_and(|sizing| sizing.height == Size::Auto);
    let content_width = sizing.is_some_and(|sizing| sizing.width == Size::Auto);
    let frame_width = if content_width {
        Size::Auto
    } else {
        Size::Percent(100.0)
    };
    let max_height = sizing.map_or(Size::Auto, |sizing| {
        Size::Length(u32::from(sizing.max_height))
    });
    let max_width = sizing.map_or(Size::Auto, |sizing| {
        Size::Length(u32::from(sizing.max_width))
    });
    let height = hooks.use_state(|| 0u16);
    hooks.use_hook(|| FrameMeasure(height)).0 = height;
    let body = props.scroll.position();
    let frame_height = if content_height && height.get() > 0 {
        let natural = height
            .get()
            .saturating_sub(body.viewport_height)
            .saturating_add(body.natural_height);
        Size::Length(u32::from(
            natural.min(sizing.map_or(u16::MAX, |sizing| sizing.max_height)),
        ))
    } else if content_height {
        Size::Auto
    } else {
        Size::Percent(100.0)
    };
    let padding = props.padding.unwrap_or(u16::from(height.get() >= 16));
    let children = std::mem::take(&mut props.children);
    let mut actions = std::mem::take(&mut props.actions);
    let action_hints = std::mem::take(&mut props.action_hints);
    let help = props.help_action.take();
    let identity = std::mem::take(&mut props.context);
    let has_context = !identity.is_empty();
    if actions.is_empty()
        && let Some(context) = context
    {
        actions = context
            .stack
            .snapshot()
            .surface(context.surface)
            .map(|surface| {
                surface
                    .actions
                    .presentations()
                    .into_iter()
                    .filter(|action| action.placement == Placement::Toolbar)
                    .map(|action| {
                        element!(Button(id:TargetId::for_action(&action.id),action:action.id))
                            .into_any()
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let has_actions = !actions.is_empty() || !action_hints.is_empty();
    element!(View(width:frame_width,height:frame_height,max_width,max_height,min_height:0,flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,padding,
        border_style:props.border_style.unwrap_or(BorderStyle::Round),border_color:props.border_color.unwrap_or(theme.foreground(TextRole::Accent)),background_color:theme.background(SurfaceRole::Base)) {
        View(width:100pct,min_height:0,flex_grow:if content_height {0.0_f32}else{1.0_f32},flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,row_gap:u16::from(has_context)) {
            View(width:100pct,flex_shrink:0.0_f32,column_gap:1,flex_wrap:FlexWrap::Wrap) {
                View(flex_shrink:0.0_f32,max_width:100pct) {Text(content:props.title.clone(),color:theme.foreground(TextRole::Primary),weight:Weight::Bold)}
                View(margin_left:Margin::Auto,flex_shrink:0.0_f32,max_width:100pct,column_gap:1,flex_wrap:FlexWrap::Wrap) {#(identity) #(help)}
            }
            ScrollList(id:props.scroll_target.clone(),scroll:props.scroll.clone(),keyboard_target:props.scroll_focus.clone(),grow:!content_height,align_bottom:!content_height,children)
        }
        #(has_actions.then(||element!(DialogFooter(children:actions,hints:action_hints))))
    })
}
