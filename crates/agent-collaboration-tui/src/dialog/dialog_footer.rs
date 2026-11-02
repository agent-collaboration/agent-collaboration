use iocraft::{ComponentDrawer, Hook, prelude::*};
struct FooterRowsMeasure(State<u16>);
impl Hook for FooterRowsMeasure {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if self.0.get() != drawer.size().height {
            self.0.set(drawer.size().height);
        }
    }
}
#[derive(Props)]
struct FooterRowsProps<'a> {
    #[iocraft(required)]
    height: State<u16>,
    children: Vec<AnyElement<'a>>,
}
#[component]
fn FooterRows<'a>(props: &mut FooterRowsProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    hooks.use_hook(|| FooterRowsMeasure(props.height)).0 = props.height;
    match props.children.first_mut() {
        Some(child) => child.into(),
        None => element!(View).into_any(),
    }
}
#[derive(Props)]
pub struct DialogFooterProps<'a> {
    pub children: Vec<AnyElement<'a>>,
    pub hints: Vec<AnyElement<'a>>,
}
#[component]
pub fn DialogFooter<'a>(
    props: &mut DialogFooterProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let action_rows = hooks.use_state(|| 0u16);
    let hint_rows = hooks.use_state(|| 0u16);
    let children = std::mem::take(&mut props.children);
    let hints = std::mem::take(&mut props.hints);
    let height = if (!children.is_empty() && action_rows.get() == 0)
        || (!hints.is_empty() && hint_rows.get() == 0)
    {
        Size::Auto
    } else {
        Size::Length(u32::from(action_rows.get()) + u32::from(hint_rows.get()) + 1)
    };
    element!(View(width:100pct,height,padding_top:1,flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
        FooterRows(height:action_rows) {View(column_gap:2,flex_shrink:0.0_f32,flex_wrap:FlexWrap::Wrap) {#(children)}}
        FooterRows(height:hint_rows) {View(flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {#(hints)}}
    })
}
