//! Dialog chrome fills its allocated rectangle; OverlayHost owns placement and opacity.
use super::dialog_footer::DialogFooter;
use super::{Button, ScrollHandle, ScrollList};
use crate::{
    actions::Placement,
    focus_model::TargetId,
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
    let height = hooks.use_state(|| 0u16);
    hooks.use_hook(|| FrameMeasure(height)).0 = height;
    let padding = props.padding.unwrap_or(u16::from(height.get() >= 16));
    let children = std::mem::take(&mut props.children);
    let mut actions = std::mem::take(&mut props.actions);
    let action_hints = std::mem::take(&mut props.action_hints);
    let help = props.help_action.take();
    let identity = std::mem::take(&mut props.context);
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
    element!(View(width:100pct,height:100pct,min_height:0,flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,padding,
        border_style:props.border_style.unwrap_or(BorderStyle::Round),border_color:props.border_color.unwrap_or(theme.foreground(TextRole::Accent)),background_color:theme.background(SurfaceRole::Base)) {
        View(width:100pct,flex_shrink:0.0_f32,column_gap:1,flex_wrap:FlexWrap::Wrap) {
            View(flex_shrink:0.0_f32,max_width:100pct) {Text(content:props.title.clone(),color:theme.foreground(TextRole::Primary),weight:Weight::Bold)}
            View(margin_left:Margin::Auto,flex_shrink:0.0_f32,max_width:100pct,column_gap:1) {#(identity) #(help)}
        }
        ScrollList(id:props.scroll_target.clone(),scroll:props.scroll.clone(),keyboard_target:props.scroll_focus.clone(),grow:true,align_bottom:true,children)
        #(has_actions.then(||element!(DialogFooter(children:actions,hints:action_hints))))
    })
}
