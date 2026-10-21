//! Caller-owned keys retain button visuals without duplicate modified-Enter effects.
use crate::{DialogAction, DialogKeyboardOwner};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn caller_owned_dialog_action_does_not_also_activate_for_modified_enter() {
    for (keyboard_owner, expected_activations) in [
        (DialogKeyboardOwner::Button, 1),
        (DialogKeyboardOwner::Caller, 0),
    ] {
        let activations = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
        key.modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        sender.send(TerminalEvent::Key(key)).unwrap();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component =
            element!(CallerOwnedActionProbe(activations: activations.clone(), keyboard_owner));
        let mut exit_sent = false;
        let frames = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            component
                .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
                .map(|canvas| {
                    let frame = canvas.to_string();
                    if !exit_sent && frame.contains("Key observed") {
                        let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
                        exit.modifiers = KeyModifiers::CONTROL;
                        sender.send(TerminalEvent::Key(exit)).unwrap();
                        exit_sent = true;
                    }
                    frame
                })
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
        assert!(frames.iter().any(|frame| frame.contains("Key observed")));
        assert_eq!(activations.load(Ordering::SeqCst), expected_activations);
    }
}

#[derive(Props)]
struct CallerOwnedActionProbeProps {
    activations: Arc<AtomicUsize>,
    keyboard_owner: DialogKeyboardOwner,
}

#[component]
fn CallerOwnedActionProbe(
    props: &CallerOwnedActionProbeProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<SystemContext>();
    let mut observed = hooks.use_state(|| false);
    let mut exit_requested = hooks.use_state(|| false);
    hooks.use_terminal_events(move |event| match event {
        TerminalEvent::Key(KeyEvent {
            code: KeyCode::Enter,
            ..
        }) => observed.set(true),
        TerminalEvent::Key(KeyEvent {
            code: KeyCode::Char('d'),
            modifiers,
            ..
        }) if modifiers.contains(KeyModifiers::CONTROL) => exit_requested.set(true),
        _ => {}
    });
    if exit_requested.get() {
        system.exit();
    }
    let activations = props.activations.clone();
    element! { View {
        Text(content: if observed.get() { "Key observed" } else { "Waiting" })
        DialogAction(label: "Resume", has_focus: true, keyboard_owner: props.keyboard_owner,
            handler: move |_| { activations.fetch_add(1, Ordering::SeqCst); })
    }}
}

#[tokio::test]
async fn shared_action_pointer_matches_the_native_button_and_disabled_guard() {
    for (modal, native, enabled) in [
        (false, true, true),
        (false, false, true),
        (false, false, false),
        (true, true, true),
        (true, false, true),
        (true, false, false),
    ] {
        let activations = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component =
            element!(PointerActionProbe(activations:activations.clone(),native,enabled,modal));
        let mut click_sent = false;
        let render = component
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .for_each(|canvas| {
                let frame = canvas.to_string();
                if !click_sent {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Choose"))
                        .unwrap();
                    let byte = line.find("Choose").unwrap();
                    let column = line
                        .get(..byte)
                        .expect("matched action label starts at a UTF-8 boundary")
                        .chars()
                        .count()
                        + 1;
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            column as u16,
                            row as u16,
                        )))
                        .unwrap();
                    let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
                    exit.modifiers = KeyModifiers::CONTROL;
                    sender.send(TerminalEvent::Key(exit)).unwrap();
                    click_sent = true;
                }
                futures_util::future::ready(())
            });
        tokio::time::timeout(std::time::Duration::from_secs(2), render)
            .await
            .unwrap();
        assert_eq!(
            activations.load(Ordering::SeqCst),
            usize::from(enabled),
            "modal={modal}, native={native}, enabled={enabled}"
        );
    }
}

#[derive(Props)]
struct PointerActionProbeProps {
    activations: Arc<AtomicUsize>,
    native: bool,
    enabled: bool,
    modal: bool,
}

#[component]
fn PointerActionProbe(
    props: &PointerActionProbeProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<SystemContext>();
    let mut exit_requested = hooks.use_state(|| false);
    hooks.use_terminal_events(move |event| {
        if matches!(event,TerminalEvent::Key(KeyEvent{code:KeyCode::Char('d'),modifiers,..}) if modifiers.contains(KeyModifiers::CONTROL)) {
            exit_requested.set(true);
        }
    });
    if exit_requested.get() {
        system.exit();
    }
    let activations = props.activations.clone();
    let action = if props.native {
        element!(Button(handler:move |_|{activations.fetch_add(1,Ordering::SeqCst);}){Text(content:"Choose")}).into_any()
    } else {
        element!(DialogAction(label:"Choose",enabled:props.enabled,keyboard_owner:DialogKeyboardOwner::Caller,
            handler:move |_|{activations.fetch_add(1,Ordering::SeqCst);})).into_any()
    };
    let body = element!(View(column_gap:2){Text(content:"Prefix") #(action)}).into_any();
    if props.modal {
        let dialog=element!(crate::DialogFrame(width:120u16,height:36u16,title:"Pointer probe".to_owned()) {#(body)}).into_any();
        element!(crate::DialogHost(width:120u16,height:36u16,dialog:Some(dialog))).into_any()
    } else {
        body
    }
}

#[derive(Props)]
struct ActivationParityProbeProps {
    activations: Arc<AtomicUsize>,
    native: bool,
    blocked: bool,
}

#[component]
fn ActivationParityProbe(
    props: &ActivationParityProbeProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let mut acknowledged = hooks.use_state(|| false);
    let mut closed = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();
    hooks.use_terminal_events(move |event| match event {
        TerminalEvent::Key(KeyEvent {
            code: KeyCode::Char('q'),
            ..
        }) => closed.set(true),
        _ => acknowledged.set(true),
    });
    if closed.get() {
        system.exit();
    }
    let activations = props.activations.clone();
    let button = if props.native {
        element!(Button(has_focus:true,handler:move |_|{activations.fetch_add(1,Ordering::SeqCst);}){Text(content:"Choose")}).into_any()
    } else {
        element!(crate::DialogButton(has_focus:true,handler:move |_|{activations.fetch_add(1,Ordering::SeqCst);}){Text(content:"Choose")}).into_any()
    };
    element! {View(flex_direction:FlexDirection::Column) {
        ContextProvider(value:Context::owned(if props.blocked {crate::DialogInputScope::Blocked}else {crate::DialogInputScope::Active})) {#(button)}
        Text(content:if acknowledged.get() {"Acknowledged"}else {"Ready"})
    }}
}

#[tokio::test]
async fn dialog_button_preserves_native_activation_and_blocks_scoped_input() {
    let mut cases = Vec::new();
    for code in [KeyCode::Enter, KeyCode::Char(' ')] {
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            cases.push((
                TerminalEvent::Key(KeyEvent::new(kind, code)),
                usize::from(kind != KeyEventKind::Release),
            ));
        }
    }
    for button in [MouseButton::Left, MouseButton::Right] {
        cases.push((
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(button),
                1,
                0,
            )),
            1,
        ));
    }
    for (event, expected) in cases {
        for (native, blocked) in [(true, false), (false, false), (false, true)] {
            let activations = Arc::new(AtomicUsize::new(0));
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let mut component =
                element!(ActivationParityProbe(activations:activations.clone(),native,blocked));
            let mut sent = false;
            let mut exited = false;
            let rendering = component
                .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
                .for_each(|canvas| {
                    let frame = canvas.to_string();
                    if !sent {
                        sender.send(event.clone()).unwrap();
                        sent = true;
                    } else if !exited && frame.contains("Acknowledged") {
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Char('q'),
                            )))
                            .unwrap();
                        exited = true;
                    }
                    futures_util::future::ready(())
                });
            tokio::time::timeout(std::time::Duration::from_secs(2), rendering)
                .await
                .unwrap();
            assert_eq!(
                activations.load(Ordering::SeqCst),
                if blocked { 0 } else { expected },
                "native={native},blocked={blocked},event={event:?}"
            );
        }
    }
}
