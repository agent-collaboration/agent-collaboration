//! Reveal the actual focused layout within the one measured body viewport.
use iocraft::{ComponentDrawer, Hook, prelude::*};

#[derive(Clone, Copy)]
pub(crate) struct DialogBodyScroll {
    pub top: State<i16>,
    pub viewport_height: State<u16>,
    pub natural_height: State<u16>,
    pub offset: State<u16>,
}

struct FocusRevealHook {
    scroll: Option<DialogBodyScroll>,
    focused: bool,
    last_target: Option<(i32, u16, u16, u16)>,
}
impl Hook for FocusRevealHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let Some(mut scroll) = self.scroll.filter(|_| self.focused) else {
            self.last_target = None;
            return;
        };
        let height = i32::from(scroll.viewport_height.get());
        if height == 0 {
            return;
        }
        let top = i32::from(drawer.canvas_position().y) - i32::from(scroll.top.get());
        let target = (
            top + i32::from(scroll.offset.get()),
            drawer.size().height,
            scroll.viewport_height.get(),
            scroll.natural_height.get(),
        );
        if self.last_target == Some(target) {
            return;
        }
        self.last_target = Some(target);
        let bottom = top + i32::from(drawer.size().height);
        let displacement = if top < 0 {
            top
        } else if bottom > height {
            if i32::from(drawer.size().height) > height {
                top
            } else {
                bottom - height
            }
        } else {
            0
        };
        let maximum = scroll
            .natural_height
            .get()
            .saturating_sub(scroll.viewport_height.get());
        let next =
            (i32::from(scroll.offset.get()) + displacement).clamp(0, i32::from(maximum)) as u16;
        if next != scroll.offset.get() {
            scroll.offset.set(next);
        }
    }
}

pub(crate) fn use_focus_reveal(hooks: &mut Hooks<'_, '_>, focused: bool) {
    let scroll = hooks
        .try_use_context::<DialogBodyScroll>()
        .map(|scroll| *scroll);
    let hook = hooks.use_hook(move || FocusRevealHook {
        scroll,
        focused,
        last_target: None,
    });
    hook.scroll = scroll;
    hook.focused = focused;
}

/// Reveal a caller-owned focus target without adding field or selector chrome.
#[derive(Props)]
pub struct FocusRevealProps<'a> {
    pub has_focus: bool,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn FocusReveal<'a>(
    props: &mut FocusRevealProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    use_focus_reveal(&mut hooks, props.has_focus);
    let children = std::mem::take(&mut props.children);
    element! {View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {#(children)}}
}
