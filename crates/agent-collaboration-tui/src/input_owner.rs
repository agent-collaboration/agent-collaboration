//! The application's single terminal subscription and event-time routing owner.
use crate::{
    actions::ActionId,
    focus_model::{FocusDirection, InputUse, TargetId, TargetSpec},
    hit_regions::{HitRegionKind, use_frame_publication},
    surface_handle::{SurfaceContext, SurfaceStackHandle},
    surface_stack::{OverlayId, OverlayKind, SurfaceId, SurfaceStack},
};
use iocraft::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Routed {
    Action(ActionId),
    Text(TargetId),
    OverlayOpened(OverlayId),
    OverlayClosed(OverlayId),
    Quit,
    Ignored,
}
#[derive(Props)]
pub struct InputOwnerProps<'a> {
    #[iocraft(required)]
    pub stack: SurfaceStackHandle,
    pub width: u16,
    pub height: u16,
    #[iocraft(required)]
    pub quit_action: ActionId,
    pub on_routed: Handler<Routed>,
    pub children: Vec<AnyElement<'a>>,
}
#[component]
pub fn InputOwner<'a>(
    props: &mut InputOwnerProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let stack = props.stack;
    let mut initialized = hooks.use_ref(|| false);
    if !initialized.get() {
        stack.set_viewport(props.width, props.height);
        initialized.set(true);
    }
    use_frame_publication(&mut hooks, stack.hit_regions());
    let quit_action = props.quit_action.clone();
    let on_routed = props.on_routed.clone();
    hooks.use_terminal_events(move |event| {
        let snapshot = stack.snapshot();
        let routed = route_event(stack, &snapshot, &quit_action, event);
        on_routed(routed);
    });
    let (width, height) = stack.viewport();
    let children = std::mem::take(&mut props.children);
    element! {
        View(width,height,overflow:Overflow::Hidden) {
            ContextProvider(value:Context::owned(SurfaceContext { stack,surface:SurfaceId::Screen })) { #(children) }
        }
    }
}

fn run_action(stack: &SurfaceStack, id: &ActionId, surface: SurfaceId) -> Routed {
    if let Some(handler) = stack
        .surface(surface)
        .and_then(|surface| surface.actions.invocation(id))
    {
        handler(());
        Routed::Action(id.clone())
    } else {
        Routed::Ignored
    }
}
fn route_event(
    handle: SurfaceStackHandle,
    stack: &SurfaceStack,
    quit: &ActionId,
    event: TerminalEvent,
) -> Routed {
    match event {
        TerminalEvent::Resize(width, height) => {
            handle.set_viewport(width, height);
            Routed::Ignored
        }
        TerminalEvent::Key(key) if key.kind == KeyEventKind::Release => Routed::Ignored,
        TerminalEvent::Key(key) if key.kind == KeyEventKind::Repeat && !repeatable(key.code) => {
            Routed::Ignored
        }
        TerminalEvent::Key(key)
            if key.kind == KeyEventKind::Press
                && key.modifiers == KeyModifiers::CONTROL
                && matches!(key.code,KeyCode::Char(character) if character.eq_ignore_ascii_case(&'c')) =>
        {
            if let Some(handler) = stack
                .surface(SurfaceId::Screen)
                .and_then(|surface| surface.actions.invocation(quit))
            {
                handler(());
                Routed::Quit
            } else {
                Routed::Ignored
            }
        }
        TerminalEvent::Key(key) => route_key(handle, stack, key),
        TerminalEvent::BracketedPaste(value) => {
            focused_input(handle, stack, TerminalEvent::BracketedPaste(value))
        }
        TerminalEvent::FullscreenMouse(mouse) => route_mouse(handle, stack, mouse),
        _ => Routed::Ignored,
    }
}
fn repeatable(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Char(_)
    )
}
fn deliver(handle: SurfaceStackHandle, target: &TargetSpec, event: TerminalEvent) -> bool {
    match target.input.call(event) {
        InputUse::Unused => false,
        InputUse::Used => true,
        InputUse::Scrolled => {
            handle.hit_regions().invalidate();
            true
        }
    }
}
fn focused_input(handle: SurfaceStackHandle, stack: &SurfaceStack, event: TerminalEvent) -> Routed {
    if let Some(target) = stack
        .top()
        .focus
        .focused()
        .filter(|target| target.is_enabled())
        && deliver(handle, target, event)
    {
        Routed::Text(target.id.clone())
    } else {
        Routed::Ignored
    }
}
fn route_key(handle: SurfaceStackHandle, stack: &SurfaceStack, key: KeyEvent) -> Routed {
    if key.kind == KeyEventKind::Press
        && key.code == KeyCode::Enter
        && key.modifiers == KeyModifiers::CONTROL
    {
        if let Some(dropdown) = stack
            .overlays()
            .last()
            .filter(|overlay| matches!(overlay.kind, OverlayKind::Dropdown { .. }))
            && let SurfaceId::Overlay(parent_id) = dropdown.parent
            && stack
                .overlays()
                .iter()
                .any(|overlay| overlay.id == parent_id && overlay.kind == OverlayKind::Dialog)
        {
            handle.close_top();
            let parent = handle.snapshot();
            return parent
                .top()
                .primary_action
                .as_ref()
                .map_or(Routed::Ignored, |id| {
                    run_action(&parent, id, parent.top_id())
                });
        }
        if stack
            .overlays()
            .last()
            .is_some_and(|overlay| overlay.kind == OverlayKind::Dialog)
        {
            return stack
                .top()
                .primary_action
                .as_ref()
                .map_or(Routed::Ignored, |id| run_action(stack, id, stack.top_id()));
        }
    }
    if let Some(target) = stack
        .top()
        .focus
        .focused()
        .filter(|target| target.is_enabled())
    {
        if deliver(handle, target, TerminalEvent::Key(key.clone())) {
            return Routed::Text(target.id.clone());
        }
        if key.kind == KeyEventKind::Press
            && key.code == KeyCode::Enter
            && key.modifiers.is_empty()
            && let Some(action) = &target.activation
        {
            return run_action(stack, action, stack.top_id());
        }
    }
    if key.kind == KeyEventKind::Press
        && key.code == KeyCode::Esc
        && stack.top_id() != SurfaceId::Screen
    {
        return handle
            .close_top()
            .map_or(Routed::Ignored, Routed::OverlayClosed);
    }
    if let Some((id, handler)) = stack.top().actions.key_invocation(&key) {
        handler(());
        return Routed::Action(id);
    }
    let direction = match key.code {
        KeyCode::Tab if key.modifiers.is_empty() => Some((FocusDirection::Next, false)),
        KeyCode::BackTab => Some((FocusDirection::Previous, false)),
        KeyCode::Tab if key.modifiers == KeyModifiers::SHIFT => {
            Some((FocusDirection::Previous, false))
        }
        KeyCode::Down | KeyCode::Right => Some((FocusDirection::Next, true)),
        KeyCode::Up | KeyCode::Left => Some((FocusDirection::Previous, true)),
        _ => None,
    };
    if let Some((direction, group)) = direction {
        handle.update(|stack| {
            if group {
                stack.top_mut().focus.traverse_group(direction)
            } else {
                stack.top_mut().focus.traverse(direction)
            }
        });
    }
    Routed::Ignored
}
fn route_mouse(
    handle: SurfaceStackHandle,
    stack: &SurfaceStack,
    mouse: FullscreenMouseEvent,
) -> Routed {
    let Some(map) = handle.hit_regions().published() else {
        return Routed::Ignored;
    };
    let surface = stack.top_id();
    let point = (mouse.column, mouse.row);
    let Some(inside) = map.surface_contains(surface, point) else {
        return Routed::Ignored;
    };
    if !inside {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && stack.overlays().last().is_some_and(|overlay| {
                matches!(
                    overlay.kind,
                    OverlayKind::Menu | OverlayKind::Dropdown { .. }
                )
            })
        {
            return handle
                .close_top()
                .map_or(Routed::Ignored, Routed::OverlayClosed);
        }
        return Routed::Ignored;
    }
    let kind = if matches!(
        mouse.kind,
        MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    ) {
        HitRegionKind::ScrollArea
    } else {
        HitRegionKind::Target
    };
    let target = map.target_at(surface, point, kind).and_then(|id| {
        stack
            .top()
            .focus
            .targets()
            .iter()
            .find(|target| target.id == *id && target.is_enabled())
    });
    if matches!(mouse.kind, MouseEventKind::Moved) {
        handle.set_hovered(target.map(|target| (surface, target.id.clone())));
    }
    let Some(target) = target else {
        return Routed::Ignored;
    };
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        handle.focus(surface, &target.id);
    }
    if deliver(
        handle,
        target,
        TerminalEvent::FullscreenMouse(mouse.clone()),
    ) {
        return Routed::Text(target.id.clone());
    }
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(action) = &target.activation
    {
        return run_action(stack, action, surface);
    }
    Routed::Ignored
}
