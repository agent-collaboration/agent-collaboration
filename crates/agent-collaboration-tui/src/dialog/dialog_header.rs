//! Shared chrome fits typed identity content before reserving its pinned rows.
use crate::dialog::pinned_context::{PinnedContext, fit_pinned_text};
use crate::foundation::theme::{TextRole, Theme};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::UnicodeWidthStr;
const MINIMUM_BODY_ROWS: u16 = 3;
struct HeaderMeasure {
    width: State<u16>,
    height: Option<State<u16>>,
}
impl Hook for HeaderMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.width.get() != drawer.size().width {
            self.width.set(drawer.size().width);
        }
        if let Some(mut height) = self.height
            && height.get() != drawer.size().height
        {
            height.set(drawer.size().height);
        }
    }
}
#[derive(Props)]
struct MeasuredHelpProps<'a> {
    #[iocraft(required)]
    width: State<u16>,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn MeasuredHelp<'a>(
    props: &mut MeasuredHelpProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    hooks
        .use_hook(|| HeaderMeasure {
            width: props.width,
            height: None,
        })
        .width = props.width;
    let children = std::mem::take(&mut props.children);
    element!(View(flex_shrink:0.0_f32) {#(children)})
}
#[derive(Props)]
pub(super) struct DialogHeaderProps<'a> {
    pub title: String,
    pub theme: Theme,
    pub context: Option<PinnedContext>,
    pub help: Option<AnyElement<'a>>,
    #[iocraft(required)]
    pub height: State<u16>,
    pub width_hint: u16,
    pub content_width: bool,
    /// Rows shared by the title, context and body, after other frame chrome.
    pub available_rows: u16,
}
#[component]
pub(super) fn DialogHeader<'a>(
    props: &mut DialogHeaderProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let width = hooks.use_state(|| 0u16);
    let help_width = hooks.use_state(|| 0u16);
    let measure = hooks.use_hook(|| HeaderMeasure {
        width,
        height: Some(props.height),
    });
    measure.width = width;
    measure.height = Some(props.height);
    // Content-sized chrome may grow back after a narrower resize. Deciding
    // against its previous draw width would permanently hide a fitting Help.
    let available_width = if props.content_width || width.get() == 0 {
        props.width_hint
    } else {
        width.get()
    };
    let context = props.context.take();
    let has_context = context.is_some();
    let help = props.help.take();
    let identity_width = context.as_ref().map_or(0, PinnedContext::natural_width);
    let needed = props.title.width() + identity_width + usize::from(has_context);
    let fits_inline = needed <= usize::from(available_width);
    let separate = has_context && !fits_inline && props.available_rows >= 2 + MINIMUM_BODY_ROWS;
    let show_help = help.is_some()
        && !separate
        && (!has_context || fits_inline)
        && (available_width == 0
            || needed + usize::from(help_width.get()) < usize::from(available_width));
    let minimum_context = context
        .as_ref()
        .and_then(|context| {
            context
                .tag
                .as_ref()
                .map(|tag| tag.width() + 2 + if context.text.is_empty() { 0 } else { 2 })
        })
        .unwrap_or(usize::from(has_context))
        .min(usize::from(available_width));
    let title_width = if has_context && !fits_inline && !separate {
        usize::from(available_width).saturating_sub(minimum_context + 1)
    } else {
        usize::from(available_width)
    };
    let title = fit_pinned_text(&props.title, title_width);
    let context_budget = if separate {
        usize::from(available_width)
    } else {
        usize::from(available_width).saturating_sub(
            title.width()
                + usize::from(has_context && !title.is_empty())
                + if show_help {
                    usize::from(help_width.get()) + 1
                } else {
                    0
                },
        )
    };
    let identity=context.map(|context| {
        let (text,tag)=context.fitted(context_budget);
        element!(View(min_width:0,flex_shrink:0.0_f32,column_gap:u16::from(!text.is_empty()&&tag.is_some()),height:1,overflow:Overflow::Hidden) {
            Text(content:text,wrap:TextWrap::NoWrap,color:props.theme.foreground(TextRole::Text))
            #(tag.map(|content|element!(Text(content,wrap:TextWrap::NoWrap,color:props.theme.foreground(TextRole::Muted)))))
        }).into_any()
    });
    // Explicit pinned height contributes every row to content-sized body flex.
    let pinned_rows = if separate { 2u16 } else { 1u16 };
    element!(View(width:100pct,height:pinned_rows,min_height:1,flex_shrink:0.0_f32,column_gap:1,align_items:AlignItems::FlexStart,flex_wrap:if separate {FlexWrap::Wrap}else{FlexWrap::NoWrap},overflow:Overflow::Hidden) {
        #((!title.is_empty()).then(||element!(View(flex_shrink:0.0_f32,max_width:100pct,height:1,overflow:Overflow::Hidden) {Text(content:title,wrap:TextWrap::NoWrap,color:props.theme.foreground(TextRole::Primary),weight:Weight::Bold)})))
        View(width:if separate {Size::Percent(100.0)}else{Size::Auto},margin_left:Margin::Auto,min_width:0,flex_shrink:1.0_f32,height:1,align_items:AlignItems::FlexStart,column_gap:u16::from(show_help&&has_context),overflow:Overflow::Hidden) {
            #(identity)
            View(width:if show_help {Size::Auto}else{Size::Length(0)},flex_shrink:0.0_f32,height:1,overflow:Overflow::Hidden) {MeasuredHelp(width:help_width) {#(help)}}
        }
    })
}

#[cfg(test)]
#[path = "dialog_header_layout_test.rs"]
mod tests;
