//! Pinned identity uses its own row when the frame can retain three body rows.
use crate::theme::{SurfaceRole, TextRole, Theme, UiGlyph};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
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
struct NaturalHeaderContentProps<'a> {
    #[iocraft(required)]
    width: State<u16>,
    height: Option<State<u16>>,
    max_width: Size,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn NaturalHeaderContent<'a>(
    props: &mut NaturalHeaderContentProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let measure = hooks.use_hook(|| HeaderMeasure {
        width: props.width,
        height: props.height,
    });
    measure.width = props.width;
    measure.height = props.height;
    let children = std::mem::take(&mut props.children);
    element!(View(max_width:props.max_width,flex_shrink:0.0_f32,flex_wrap:FlexWrap::NoWrap) {#(children)})
}
#[derive(Props)]
struct HeaderContextProps<'a> {
    #[iocraft(required)]
    natural_width: State<u16>,
    #[iocraft(required)]
    natural_height: State<u16>,
    available_width: u16,
    separate: bool,
    theme: Theme,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn HeaderContext<'a>(
    props: &mut HeaderContextProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let width = hooks.use_state(|| 0u16);
    let measure = hooks.use_hook(|| HeaderMeasure {
        width,
        height: None,
    });
    measure.width = width;
    let children = std::mem::take(&mut props.children);
    let preferred = if props.natural_width.get() > 0 {
        Size::Length(u32::from(props.natural_width.get()))
    } else {
        Size::Auto
    };
    let clipped = !props.separate
        && width.get() > 0
        && (props.natural_width.get() > width.get() || props.natural_height.get() > 1);
    element!(View(width:if props.separate {Size::Percent(100.0)}else{preferred},min_width:0,flex_shrink:if props.separate {0.0_f32}else{1.0_f32},height:if props.separate {Size::Auto}else{Size::Length(1)},align_items:AlignItems::FlexStart,overflow:Overflow::Hidden) {
        // Measure the one context subtree at row width, independently of its
        // inline clip, so the caller's tag reservation and wrapping stay intact.
        View(width:if props.available_width>0 {Size::Length(u32::from(props.available_width))}else{Size::Auto},flex_shrink:0.0_f32,align_items:AlignItems::FlexStart) {
            NaturalHeaderContent(width:props.natural_width,height:Some(props.natural_height),max_width:100pct,children)
        }
        #(clipped.then(||element!(View(position:Position::Absolute,right:0,top:0,width:1,height:1,background_color:props.theme.background(SurfaceRole::Base)) {Text(content:UiGlyph::Ellipsis.text(),color:props.theme.foreground(TextRole::Text))})))
    })
}
#[derive(Props)]
pub(super) struct DialogHeaderProps<'a> {
    pub title: String,
    pub theme: Theme,
    pub context: Vec<AnyElement<'a>>,
    pub help: Option<AnyElement<'a>>,
    #[iocraft(required)]
    pub height: State<u16>,
    pub width_hint: u16,
    /// Rows shared by the title, context and body, after other frame chrome.
    pub available_rows: u16,
}
#[component]
pub(super) fn DialogHeader<'a>(
    props: &mut DialogHeaderProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let width = hooks.use_state(|| 0u16);
    let context_width = hooks.use_state(|| 0u16);
    let context_height = hooks.use_state(|| 1u16);
    let help_width = hooks.use_state(|| 0u16);
    let measure = hooks.use_hook(|| HeaderMeasure {
        width,
        height: Some(props.height),
    });
    measure.width = width;
    measure.height = Some(props.height);
    let available_width = if width.get() > 0 {
        width.get()
    } else {
        props.width_hint
    };
    let context = std::mem::take(&mut props.context);
    let has_context = !context.is_empty();
    let help = props.help.take();
    let identity_width = if has_context { context_width.get() } else { 0 };
    let identity_rows = if has_context {
        context_height.get().max(1)
    } else {
        0
    };
    let needed = props.title.width() + usize::from(identity_width) + usize::from(has_context);
    let fits_inline = identity_rows <= 1 && needed <= usize::from(available_width);
    let separate = has_context
        && !fits_inline
        && identity_width <= available_width
        && props.available_rows >= identity_rows.saturating_add(1 + MINIMUM_BODY_ROWS);
    let show_help = help.is_some()
        && !separate
        && (!has_context || fits_inline)
        && (available_width == 0
            || needed + usize::from(help_width.get()) < usize::from(available_width));
    let title = fit_header_title(&props.title, usize::from(available_width));
    element!(View(width:100pct,height:if separate {Size::Auto}else{Size::Length(1)},min_height:1,flex_shrink:0.0_f32,column_gap:1,align_items:AlignItems::FlexStart,flex_wrap:if separate {FlexWrap::Wrap}else{FlexWrap::NoWrap},overflow:Overflow::Hidden) {
        View(flex_shrink:0.0_f32,max_width:100pct,height:1,overflow:Overflow::Hidden) {Text(content:title,wrap:TextWrap::NoWrap,color:props.theme.foreground(TextRole::Primary),weight:Weight::Bold)}
        View(width:if separate {Size::Percent(100.0)}else{Size::Auto},margin_left:Margin::Auto,min_width:0,flex_shrink:if separate {0.0_f32}else{1.0_f32},height:if separate {Size::Auto}else{Size::Length(1)},align_items:AlignItems::FlexStart,column_gap:u16::from(show_help&&has_context),overflow:Overflow::Hidden) {
            #(has_context.then(||element!(HeaderContext(natural_width:context_width,natural_height:context_height,available_width,separate,theme:props.theme,children:context))))
            View(width:if show_help {Size::Auto}else{Size::Length(0)},flex_shrink:0.0_f32,height:1,overflow:Overflow::Hidden) {NaturalHeaderContent(width:help_width) {#(help)}}
        }
    })
}
fn fit_header_title(content: &str, width: usize) -> String {
    if width == 0 || content.width() <= width {
        return content.to_owned();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in content.chars() {
        let cells = character.width().unwrap_or(0);
        if used + cells >= width {
            break;
        }
        result.push(character);
        used += cells;
    }
    result.push_str(UiGlyph::Ellipsis.text());
    result
}
