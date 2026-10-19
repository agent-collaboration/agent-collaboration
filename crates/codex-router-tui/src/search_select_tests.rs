use crate::*;
use futures_util::StreamExt;
use iocraft::prelude::*;

#[derive(Props)]
struct ChoiceProbeProps {
    marker: String,
}
#[component]
fn ChoiceProbe(props: &ChoiceProbeProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut choice = hooks.use_state(|| {
        let mut state = SearchSelectState::Closed;
        state.open(20u32);
        state
    });
    let mut selection = hooks.use_state(|| 0u32);
    let mut closed = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();
    let options = vec![
        ChoiceOption::available(10u32, "Local"),
        ChoiceOption::available(20, "Remote"),
    ];
    hooks.use_terminal_events({
        let options = options.clone();
        move |event| {
            if let TerminalEvent::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                ..
            }) = event
            {
                if code == KeyCode::Char('q') {
                    closed.set(true);
                    return;
                }
                if let ChoiceTransition::Selected(key) = choice.write().handle_key(code, &options) {
                    selection.set(key);
                }
            }
        }
    });
    if closed.get() {
        system.exit();
    }
    element! {
        View(width:40,height:12,flex_direction:FlexDirection::Column) {
            Text(content:format!("{} selected:{}",props.marker,selection.get()))
            SearchSelect<u32>(state:choice.read().clone(),options,on_select:move |key| {let mut selection=selection; selection.set(key);})
        }
    }
}

#[tokio::test]
async fn real_inline_component_pointer_and_keyboard_select_the_same_key() {
    for pointer in [false, true] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut ui = element!(ChoiceProbe(marker:"probe".to_owned()));
        let frames = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                last = frame.clone();
                if phase == 0 && frame.contains("Remote") {
                    let event = if pointer {
                        let row = frame
                            .lines()
                            .position(|line| line.contains("Remote"))
                            .unwrap();
                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            3,
                            row as u16,
                        ))
                    } else {
                        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter))
                    };
                    sender.send(event).unwrap();
                    phase = 1;
                } else if phase == 1 && frame.contains("selected:20") {
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Char('q'),
                        )))
                        .unwrap();
                    phase = 2;
                }
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
        assert!(result.is_ok(), "pointer={pointer} phase={phase} {last}");
        assert_eq!(phase, 2);
    }
}
