//! A single native flex row gives context priority over the optional Help hint.
use crate::theme::{SurfaceRole, TextRole, Theme, UiGlyph};
use iocraft::{ComponentDrawer, Hook, prelude::*};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
struct HeaderWidth(State<u16>);
impl Hook for HeaderWidth {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.0.get() != drawer.size().width {
            self.0.set(drawer.size().width);
        }
    }
}
struct NaturalHeaderMeasure {
    width: State<u16>,
    wrapped: Option<State<bool>>,
}
impl Hook for NaturalHeaderMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.width.get() != drawer.size().width {
            self.width.set(drawer.size().width);
        }
        if let Some(mut wrapped) = self.wrapped {
            let current = drawer.size().height > 1;
            if wrapped.get() != current {
                wrapped.set(current);
            }
        }
    }
}
#[derive(Props)]
struct NaturalHeaderContentProps<'a> {
    #[iocraft(required)]
    width: State<u16>,
    wrapped: Option<State<bool>>,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn NaturalHeaderContent<'a>(
    props: &mut NaturalHeaderContentProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let measure = hooks.use_hook(|| NaturalHeaderMeasure {
        width: props.width,
        wrapped: props.wrapped,
    });
    measure.width = props.width;
    measure.wrapped = props.wrapped;
    let children = std::mem::take(&mut props.children);
    element!(View(flex_shrink:0.0_f32,flex_wrap:FlexWrap::NoWrap) {#(children)})
}
#[derive(Props)]
struct HeaderContextProps<'a> {
    #[iocraft(required)]
    natural_width: State<u16>,
    #[iocraft(required)]
    wrapped: State<bool>,
    theme: Theme,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn HeaderContext<'a>(
    props: &mut HeaderContextProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let width = hooks.use_state(|| 0u16);
    hooks.use_hook(|| HeaderWidth(width)).0 = width;
    let children = std::mem::take(&mut props.children);
    let clipped =
        (props.natural_width.get() > width.get() || props.wrapped.get()) && width.get() > 0;
    element!(View(min_width:0,flex_shrink:1.0_f32,height:1,overflow:Overflow::Hidden) {
        NaturalHeaderContent(width:props.natural_width,wrapped:Some(props.wrapped),children)
        #(clipped.then(||element!(View(position:Position::Absolute,right:0,top:0,width:1,height:1,background_color:props.theme.background(SurfaceRole::Base)) {Text(content:UiGlyph::Ellipsis.text(),color:props.theme.foreground(TextRole::Text))})))
    })
}
#[derive(Props)]
pub(super) struct DialogHeaderProps<'a> {
    pub title: String,
    pub theme: Theme,
    pub context: Vec<AnyElement<'a>>,
    pub help: Option<AnyElement<'a>>,
}
#[component]
pub(super) fn DialogHeader<'a>(
    props: &mut DialogHeaderProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let width = hooks.use_state(|| 0u16);
    let context_width = hooks.use_state(|| 0u16);
    let help_width = hooks.use_state(|| 0u16);
    let context_wrapped = hooks.use_state(|| false);
    hooks.use_hook(|| HeaderWidth(width)).0 = width;
    let context = std::mem::take(&mut props.context);
    let has_context = !context.is_empty();
    let help = props.help.take();
    let needed = props.title.width()
        + if has_context {
            usize::from(context_width.get())
        } else {
            0
        }
        + usize::from(help_width.get())
        + usize::from(has_context)
        + usize::from(help.is_some());
    let show_help = help.is_some()
        && (!has_context || !context_wrapped.get())
        && (width.get() == 0 || needed <= usize::from(width.get()));
    let title = fit_header_title(&props.title, usize::from(width.get()));
    element!(View(width:100pct,height:1,min_height:1,flex_shrink:0.0_f32,column_gap:1,overflow:Overflow::Hidden) {
        View(flex_shrink:0.0_f32,max_width:100pct,height:1,overflow:Overflow::Hidden) {Text(content:title,wrap:TextWrap::NoWrap,color:props.theme.foreground(TextRole::Primary),weight:Weight::Bold)}
        View(margin_left:Margin::Auto,min_width:0,flex_shrink:1.0_f32,height:1,column_gap:u16::from(show_help&&has_context),overflow:Overflow::Hidden) {
            #(has_context.then(||element!(HeaderContext(natural_width:context_width,wrapped:context_wrapped,theme:props.theme,children:context))))
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
