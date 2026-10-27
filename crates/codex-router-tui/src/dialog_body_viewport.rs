//! One in-flow measured content tree, clipped and displaced inside its viewport.
use crate::UseDialogInteraction;
use iocraft::{ComponentDrawer, Hook, prelude::*};

/// Keep caller-owned paging separate from body scrolling under broadcast events.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogScrollKeys {
    #[default]
    Disabled,
    Arrows,
    /// Keep caller-owned arrows for fields/choices while long content can page.
    Pages,
    Navigation,
}
impl DialogScrollKeys {
    fn accepts(self, code: KeyCode) -> bool {
        match self {
            Self::Disabled => false,
            Self::Arrows => matches!(
                code,
                KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
            ),
            Self::Pages => matches!(
                code,
                KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
            ),
            Self::Navigation => matches!(
                code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ),
        }
    }
}

struct BodyMeasureHook {
    height: State<u16>,
    top: Option<State<i16>>,
}
impl Hook for BodyMeasureHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        if let Some(mut top) = self.top {
            let measured = drawer.canvas_position().y;
            if top.try_get() != Some(measured) {
                top.set(measured);
            }
        }
        let measured = drawer.size().height;
        if self.height.try_get() != Some(measured) {
            self.height.set(measured);
        }
    }
}

#[derive(Props)]
struct MeasuredBodyProps<'a> {
    height: Option<State<u16>>,
    offset: u16,
    children: Vec<AnyElement<'a>>,
}

#[component]
fn MeasuredBody<'a>(
    props: &mut MeasuredBodyProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    if let Some(height) = props.height {
        hooks.use_hook(move || BodyMeasureHook { height, top: None });
    }
    let children = std::mem::take(&mut props.children);
    element! {
        View(position: Position::Relative, top: -i32::from(props.offset), flex_direction: FlexDirection::Column, flex_shrink: 0.0_f32) {
            #(children)
        }
    }
}

#[derive(Props)]
pub struct DialogBodyViewportProps<'a> {
    pub scroll_keys: DialogScrollKeys,
    pub children: Vec<AnyElement<'a>>,
}

#[component]
pub fn DialogBodyViewport<'a>(
    props: &mut DialogBodyViewportProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let natural_height = hooks.use_state(|| 0u16);
    let viewport_height = hooks.use_state(|| 0u16);
    let mut offset = hooks.use_state(|| 0u16);
    let viewport_top = hooks.use_state(|| 0i16);
    let scroll_keys = props.scroll_keys;
    hooks.use_hook(move || BodyMeasureHook {
        height: viewport_height,
        top: Some(viewport_top),
    });
    let maximum_offset = natural_height.get().saturating_sub(viewport_height.get());
    let layout_offset = offset.get().min(maximum_offset);
    hooks.use_dialog_events(move |event| {
        let maximum = natural_height.get().saturating_sub(viewport_height.get());
        let next = match event {
            TerminalEvent::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                ..
            }) if scroll_keys.accepts(code) => match code {
                KeyCode::Up => offset.get().saturating_sub(1),
                KeyCode::Down => offset.get().saturating_add(1),
                KeyCode::PageUp => offset.get().saturating_sub(viewport_height.get().max(1)),
                KeyCode::PageDown => offset.get().saturating_add(viewport_height.get().max(1)),
                KeyCode::Home => 0,
                KeyCode::End => maximum,
                _ => return,
            },
            _ => return,
        }
        .min(maximum);
        if next != offset.get() {
            offset.set(next);
        }
    });
    hooks.use_dialog_local_events(move |event| {
        let maximum = natural_height.get().saturating_sub(viewport_height.get());
        let next = match event {
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            }) => offset.get().saturating_add(3).min(maximum),
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            }) => offset.get().saturating_sub(3),
            _ => return,
        };
        if next != offset.get() {
            offset.set(next);
        }
    });
    let children = std::mem::take(&mut props.children);
    let visible_height = viewport_height.get();
    let thumb_height = if natural_height.get() == 0 {
        0
    } else {
        (u32::from(visible_height) * u32::from(visible_height) / u32::from(natural_height.get()))
            .max(1)
            .min(u32::from(visible_height)) as u16
    };
    let thumb_top = if maximum_offset == 0 {
        0
    } else {
        (u32::from(offset.get().min(maximum_offset))
            * u32::from(visible_height.saturating_sub(thumb_height))
            / u32::from(maximum_offset)) as u16
    };
    let scrollbar=(maximum_offset>0).then(||element! {
        View(position:Position::Absolute,right:0,top:0,width:1,height:u32::from(visible_height),flex_direction:FlexDirection::Column) {
            #((0..visible_height).map(|row|element!(Text(content:if row>=thumb_top&&row<thumb_top+thumb_height {"┃"}else {"│"},color:Color::DarkGrey))))
        }
    });
    element! {
        View(padding_right:1,min_height: 0,flex_shrink: 1.0_f32, overflow: Overflow::Hidden, flex_direction: FlexDirection::Column) {
            ContextProvider(value:Context::owned(crate::dialog_focus_reveal::DialogBodyScroll {top:viewport_top,viewport_height,natural_height,offset,layout_offset})) {
                View(flex_shrink: 0.0_f32, flex_direction: FlexDirection::Column) {
                    MeasuredBody(height: natural_height, offset: layout_offset, children)
                }
            }
            #(scrollbar)
        }
    }
}
