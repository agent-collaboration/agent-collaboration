use iocraft::prelude::*;
#[derive(Props)]
pub struct DialogFooterProps<'a> {
    pub children: Vec<AnyElement<'a>>,
    pub hints: Vec<AnyElement<'a>>,
}
#[component]
pub fn DialogFooter<'a>(props: &mut DialogFooterProps<'a>) -> impl Into<AnyElement<'a>> {
    let children = std::mem::take(&mut props.children);
    let hints = std::mem::take(&mut props.hints);
    element!(View(width:100pct,padding_top:1,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
        View(column_gap:2,flex_shrink:0.0_f32,flex_wrap:FlexWrap::Wrap) {#(children)}
        View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {#(hints)}
    })
}
