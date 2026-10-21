use iocraft::prelude::*;
use iocraft::{ComponentDrawer, Hook};

struct LocalPointerOrigin {
    top: State<Option<i16>>,
}

impl Hook for LocalPointerOrigin {
    fn post_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let top = Some(drawer.canvas_position().y);
        if self.top.try_get() != Some(top) {
            self.top.set(top);
        }
    }
}

/// Whether descendants may act on broadcast terminal input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogInputScope {
    #[default]
    Active,
    Blocked,
}

/// Background widgets opt into the host's input policy at their event boundary.
/// Painting an overlay alone does not consume iocraft's broadcast events.
pub trait UseDialogInteraction {
    fn dialog_input_enabled(&self) -> bool;
    fn use_dialog_events(&mut self, handler: impl FnMut(TerminalEvent) + Send + 'static);
    fn use_dialog_local_events(&mut self, handler: impl FnMut(TerminalEvent) + Send + 'static);
}

impl UseDialogInteraction for Hooks<'_, '_> {
    fn dialog_input_enabled(&self) -> bool {
        self.try_use_context::<DialogInputScope>()
            .is_none_or(|scope| *scope == DialogInputScope::Active)
    }

    fn use_dialog_events(&mut self, mut handler: impl FnMut(TerminalEvent) + Send + 'static) {
        let enabled = self.dialog_input_enabled();
        self.use_terminal_events(move |event| {
            if enabled {
                handler(event);
            }
        });
    }

    fn use_dialog_local_events(&mut self, mut handler: impl FnMut(TerminalEvent) + Send + 'static) {
        let enabled = self.dialog_input_enabled();
        let body = self
            .try_use_context::<crate::dialog_focus_reveal::DialogBodyScroll>()
            .map(|body| *body);
        let top = self.use_state(|| None);
        self.use_hook(move || LocalPointerOrigin { top });
        self.use_local_terminal_events(move |event| {
            if !enabled {
                return;
            }
            // iocraft local events use the natural layout, including clipped rows.
            // Translate back to the canvas before testing the visible body band.
            if let (Some(body), TerminalEvent::FullscreenMouse(mouse)) = (body, &event) {
                let Some(top) = top.get() else { return };
                let row = i32::from(top) + i32::from(mouse.row);
                let body_top = i32::from(body.top.get());
                if row < body_top || row >= body_top + i32::from(body.viewport_height.get()) {
                    return;
                }
            }
            handler(event);
        });
    }
}

#[derive(Props)]
pub struct DialogButtonProps<'a> {
    pub children: Vec<AnyElement<'a>>,
    pub has_focus: bool,
    pub handler: HandlerMut<'static, ()>,
}

/// Preserve native Button activation and transparent layout inside clipped dialogs.
#[component]
pub fn DialogButton<'a>(
    props: &mut DialogButtonProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    assert!(
        props.children.len() <= 1,
        "DialogButton accepts at most one child"
    );
    let has_focus = props.has_focus;
    let mut handler = props.handler.take();
    hooks.use_dialog_local_events(move |event| match event {
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
            kind: MouseEventKind::Down(_),
            ..
        }) => handler(()),
        TerminalEvent::Key(KeyEvent { code, kind, .. })
            if has_focus
                && kind != KeyEventKind::Release
                && matches!(code, KeyCode::Enter | KeyCode::Char(' ')) =>
        {
            handler(())
        }
        _ => {}
    });
    match props.children.first_mut() {
        Some(child) => child.into(),
        None => element!(View).into_any(),
    }
}
