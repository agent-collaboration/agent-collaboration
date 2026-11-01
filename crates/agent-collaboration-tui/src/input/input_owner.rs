//! The application's single terminal subscription and event-time routing owner.
use crate::{
    actions::ActionId,
    surfaces::{
        focus_model::{FocusDirection, InputUse, TargetId, TargetSpec},
        hit_regions::{HitRegionKind, use_frame_publication},
        surface_handle::{SurfaceContext, SurfaceStackHandle},
        surface_stack::{OverlayId, OverlayKind, SurfaceId, SurfaceStack},
    },
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
    /// Extra bindings of the screen's quit action that are active on every surface.
    pub global_quit_aliases: Vec<crate::foundation::KeyChord>,
    pub on_routed: Handler<Routed>,
    #[cfg(all(feature = "test-support", debug_assertions))]
    pub dispatch_receipt: Option<crate::input::input_dispatch_receipt::InputDispatchReceiptSink>,
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
    stack.reconcile_active_focus();
    use_frame_publication(&mut hooks, stack.hit_regions());
    let quit_action = props.quit_action.clone();
    let global_quit_aliases = props.global_quit_aliases.clone();
    let on_routed = props.on_routed.clone();
    #[cfg(all(feature = "test-support", debug_assertions))]
    let dispatch_receipt = props.dispatch_receipt.clone();
    #[expect(
        clippy::disallowed_methods,
        reason = "InputOwner is the only terminal subscriber (R5)"
    )]
    hooks.use_terminal_events(move |event| {
        #[cfg(all(feature = "test-support", debug_assertions))]
        let receipt_event = crate::input::input_dispatch_receipt::InputEventReadback::from(&event);
        stack.reconcile_active_focus();
        let snapshot = stack.snapshot();
        let decision = route_event(stack, &snapshot, &quit_action, &global_quit_aliases, event);
        on_routed(decision.routed);
        #[cfg(all(feature = "test-support", debug_assertions))]
        if let Some(sink) = &dispatch_receipt {
            sink.record(receipt_event, decision.facts);
        }
    });
    #[cfg(all(feature = "test-support", debug_assertions))]
    crate::input::dispatch_frame_marker::use_dispatch_frames(
        &mut hooks,
        props
            .dispatch_receipt
            .as_ref()
            .map(|sink| sink.frame_state()),
    );
    let (width, height) = stack.viewport();
    let children = std::mem::take(&mut props.children);
    element! {
        View(width,height,overflow:Overflow::Hidden) {
            ContextProvider(value:Context::owned(SurfaceContext { stack,surface:SurfaceId::Screen })) { #(children) }
        }
    }
}

struct RouteDecision {
    routed: Routed,
    #[cfg(all(feature = "test-support", debug_assertions))]
    facts: crate::input::input_dispatch_receipt::InputRouteFacts,
}
impl RouteDecision {
    fn new(
        routed: Routed,
        _surface: SurfaceId,
        _target: Option<&TargetId>,
        _action: Option<&ActionId>,
    ) -> Self {
        Self {
            routed,
            #[cfg(all(feature = "test-support", debug_assertions))]
            facts: crate::input::input_dispatch_receipt::InputRouteFacts {
                surface: _surface,
                target: _target.cloned(),
                action: _action.cloned(),
            },
        }
    }
    fn ignored(surface: SurfaceId) -> Self {
        Self::new(Routed::Ignored, surface, None, None)
    }
}
fn run_action(
    stack: &SurfaceStack,
    id: &ActionId,
    surface: SurfaceId,
    target: Option<&TargetId>,
) -> RouteDecision {
    if let Some(handler) = stack
        .surface(surface)
        .and_then(|surface| surface.actions.invocation(id))
    {
        handler(());
        RouteDecision::new(Routed::Action(id.clone()), surface, target, Some(id))
    } else {
        RouteDecision::ignored(surface)
    }
}
fn route_event(
    handle: SurfaceStackHandle,
    stack: &SurfaceStack,
    quit: &ActionId,
    aliases: &[crate::foundation::KeyChord],
    event: TerminalEvent,
) -> RouteDecision {
    match event {
        TerminalEvent::Resize(width, height) => {
            handle.set_viewport(width, height);
            RouteDecision::ignored(stack.top_id())
        }
        TerminalEvent::Key(key) if key.kind == KeyEventKind::Release => {
            RouteDecision::ignored(stack.top_id())
        }
        TerminalEvent::Key(key) if key.kind == KeyEventKind::Repeat && !repeatable(key.code) => {
            RouteDecision::ignored(stack.top_id())
        }
        TerminalEvent::Key(key)
            if key.kind == KeyEventKind::Press
                && (crate::foundation::KeyChord::normalized(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                )
                .matches(&key)
                    || declared_quit_alias(stack, quit, aliases, &key)) =>
        {
            if let Some(handler) = stack
                .surface(SurfaceId::Screen)
                .and_then(|surface| surface.actions.invocation(quit))
            {
                handler(());
                RouteDecision::new(Routed::Quit, SurfaceId::Screen, None, Some(quit))
            } else {
                RouteDecision::ignored(SurfaceId::Screen)
            }
        }
        TerminalEvent::Key(key) => route_key(handle, stack, key),
        TerminalEvent::BracketedPaste(value) => {
            focused_input(handle, stack, TerminalEvent::BracketedPaste(value))
        }
        TerminalEvent::FullscreenMouse(mouse) => route_mouse(handle, stack, mouse),
        _ => RouteDecision::ignored(stack.top_id()),
    }
}
fn declared_quit_alias(
    stack: &SurfaceStack,
    quit: &ActionId,
    aliases: &[crate::foundation::KeyChord],
    key: &KeyEvent,
) -> bool {
    aliases.iter().any(|alias| alias.matches(key))
        && stack.surface(SurfaceId::Screen).is_some_and(|screen| {
            screen.actions.definitions().iter().any(|action| {
                action.id == *quit && action.keys.iter().any(|binding| binding.matches(key))
            })
        })
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
fn focused_input(
    handle: SurfaceStackHandle,
    stack: &SurfaceStack,
    event: TerminalEvent,
) -> RouteDecision {
    if let Some(target) = stack
        .top()
        .focus
        .focused()
        .filter(|target| target.is_enabled())
        && deliver(handle, target, event)
    {
        RouteDecision::new(
            Routed::Text(target.id.clone()),
            stack.top_id(),
            Some(&target.id),
            None,
        )
    } else {
        RouteDecision::ignored(stack.top_id())
    }
}
fn route_key(handle: SurfaceStackHandle, stack: &SurfaceStack, key: KeyEvent) -> RouteDecision {
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
            return parent.top().primary_action.as_ref().map_or_else(
                || RouteDecision::ignored(parent.top_id()),
                |id| run_action(&parent, id, parent.top_id(), None),
            );
        }
        if stack
            .overlays()
            .last()
            .is_some_and(|overlay| overlay.kind == OverlayKind::Dialog)
        {
            return stack.top().primary_action.as_ref().map_or_else(
                || RouteDecision::ignored(stack.top_id()),
                |id| run_action(stack, id, stack.top_id(), None),
            );
        }
    }
    if let Some(target) = stack
        .top()
        .focus
        .focused()
        .filter(|target| target.is_enabled())
    {
        if deliver(handle, target, TerminalEvent::Key(key.clone())) {
            return RouteDecision::new(
                Routed::Text(target.id.clone()),
                stack.top_id(),
                Some(&target.id),
                None,
            );
        }
        if key.kind == KeyEventKind::Press
            && key.code == KeyCode::Enter
            && key.modifiers.is_empty()
            && let Some(action) = &target.activation
        {
            return run_action(stack, action, stack.top_id(), Some(&target.id));
        }
    }
    if key.kind == KeyEventKind::Press
        && key.code == KeyCode::Esc
        && stack.top_id() != SurfaceId::Screen
    {
        return handle.close_top().map_or_else(
            || RouteDecision::ignored(stack.top_id()),
            |id| RouteDecision::new(Routed::OverlayClosed(id), stack.top_id(), None, None),
        );
    }
    if let Some((id, handler)) = stack.top().actions.key_invocation(&key) {
        handler(());
        return RouteDecision::new(Routed::Action(id.clone()), stack.top_id(), None, Some(&id));
    }
    let direction = match key.code {
        KeyCode::Tab if key.modifiers.is_empty() => Some((FocusDirection::Next, false)),
        KeyCode::BackTab => Some((FocusDirection::Previous, false)),
        KeyCode::Tab if key.modifiers == KeyModifiers::SHIFT => {
            Some((FocusDirection::Previous, false))
        }
        KeyCode::Down | KeyCode::Right if key.modifiers.is_empty() => {
            Some((FocusDirection::Next, true))
        }
        KeyCode::Up | KeyCode::Left if key.modifiers.is_empty() => {
            Some((FocusDirection::Previous, true))
        }
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
    RouteDecision::ignored(stack.top_id())
}
fn route_mouse(
    handle: SurfaceStackHandle,
    stack: &SurfaceStack,
    mouse: FullscreenMouseEvent,
) -> RouteDecision {
    let surface = stack.top_id();
    let Some(map) = handle.hit_regions().published() else {
        return RouteDecision::ignored(surface);
    };
    let point = (mouse.column, mouse.row);
    let Some(inside) = map.surface_contains(surface, point) else {
        return RouteDecision::ignored(surface);
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
            return handle.close_top().map_or_else(
                || RouteDecision::ignored(surface),
                |id| RouteDecision::new(Routed::OverlayClosed(id), surface, None, None),
            );
        }
        return RouteDecision::ignored(surface);
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
        return RouteDecision::ignored(surface);
    };
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        handle.focus_from_pointer(surface, &target.id);
    }
    if deliver(
        handle,
        target,
        TerminalEvent::FullscreenMouse(mouse.clone()),
    ) {
        return RouteDecision::new(
            Routed::Text(target.id.clone()),
            surface,
            Some(&target.id),
            None,
        );
    }
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(action) = &target.activation
    {
        return run_action(stack, action, surface, Some(&target.id));
    }
    RouteDecision::ignored(surface)
}

#[cfg(test)]
#[path = "input_isolation_test.rs"]
mod isolation_test;
#[cfg(test)]
#[path = "global_quit_aliases_test.rs"]
mod quit_alias_test;
#[cfg(test)]
#[path = "surface_exceptions_test.rs"]
mod surface_exceptions_test;
#[cfg(test)]
#[path = "wheel_isolation_test.rs"]
mod wheel_isolation_test;
