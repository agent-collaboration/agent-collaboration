//! Dialog chrome measures content axes within OverlayHost's resolved bounds.
use super::dialog_footer::DialogFooter;
use super::dialog_header::DialogHeader;
use super::{Button, PinnedContext, ScrollHandle, ScrollList};
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
struct MeasuredFooterProps<'a> {
    #[iocraft(required)]
    height: State<u16>,
    children: Vec<AnyElement<'a>>,
    hints: Vec<AnyElement<'a>>,
}
#[component]
fn MeasuredFooter<'a>(
    props: &mut MeasuredFooterProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    hooks.use_hook(|| FrameMeasure(props.height)).0 = props.height;
    let children = std::mem::take(&mut props.children);
    let hints = std::mem::take(&mut props.hints);
    element!(View(width:100pct,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {DialogFooter(children,hints)})
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
    pub context: Option<PinnedContext>,
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
    let footer_height = hooks.use_state(|| 0u16);
    let header_height = hooks.use_state(|| 1u16);
    let frame_height = if content_height {
        Size::Auto
    } else {
        Size::Percent(100.0)
    };
    let children = std::mem::take(&mut props.children);
    let mut actions = std::mem::take(&mut props.actions);
    let action_hints = std::mem::take(&mut props.action_hints);
    let help = props.help_action.take();
    let identity = props.context.take();
    let has_context = identity.is_some();
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
    let border_style = props.border_style.unwrap_or(BorderStyle::Round);
    let border_rows = u16::from(border_style != BorderStyle::None) * 2;
    let header_chrome = header_height.get().saturating_add(border_rows);
    // Content padding follows the current body, never the previous Help frame.
    let content_extent = props
        .scroll
        .position()
        .natural_height
        .saturating_add(header_chrome)
        .saturating_add(u16::from(has_context))
        .saturating_add(if has_actions { footer_height.get() } else { 0 })
        .min(sizing.map_or(u16::MAX, |sizing| sizing.max_height));
    let padding_height = if content_height {
        content_extent
    } else {
        height.get()
    };
    let padding = props.padding.unwrap_or(u16::from(padding_height >= 16));
    // Width-dependent wrapping can outgrow the native auto-height basis.
    // Reserve the current measured body and chrome, without borrowing a prior
    // body viewport or forcing the configured maximum.
    let minimum_height = if content_height {
        content_extent
            .saturating_add(padding.saturating_mul(2))
            .min(sizing.map_or(u16::MAX, |sizing| sizing.max_height))
    } else {
        0
    };
    let body_budget = sizing
        .map_or(u16::MAX, |sizing| sizing.max_height)
        .saturating_sub(
            header_chrome
                + padding.saturating_mul(2)
                + if has_actions { footer_height.get() } else { 0 },
        );
    let context_gap = u16::from(has_context && body_budget > 1);
    let available_rows = sizing
        .map_or(u16::MAX, |sizing| sizing.max_height)
        .saturating_sub(
            border_rows
                + padding.saturating_mul(2)
                + if has_actions { footer_height.get() } else { 0 }
                + context_gap,
        );
    let width_hint = sizing
        .map_or(0, |sizing| sizing.max_width)
        .saturating_sub(border_rows + padding.saturating_mul(2));
    element!(View(width:frame_width,height:frame_height,max_width,max_height,min_height:minimum_height,flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden,padding,
        border_style,border_color:props.border_color.unwrap_or(theme.foreground(TextRole::Accent)),background_color:theme.background(SurfaceRole::Base)) {
        View(width:100pct,min_height:0,flex_grow:if content_height {0.0_f32}else{1.0_f32},flex_shrink:1.0_f32,flex_direction:FlexDirection::Column,row_gap:context_gap) {
            DialogHeader(title:props.title.clone(),theme,context:identity,help,height:header_height,width_hint,content_width,available_rows)
            ScrollList(id:props.scroll_target.clone(),scroll:props.scroll.clone(),keyboard_target:props.scroll_focus.clone(),grow:!content_height,align_bottom:!content_height,children)
        }
        #(has_actions.then(||element!(MeasuredFooter(height:footer_height,children:actions,hints:action_hints))))
    })
}
