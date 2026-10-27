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
                            u16::try_from(row).expect("fixture coordinates fit terminal cells"),
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

#[component]
fn ClippedChoiceProbe(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let selected = hooks.use_state(|| 0_u32);
    let mut acknowledged = hooks.use_state(|| 0_u32);
    let mut closed = hooks.use_state(|| false);
    let mut body_rows = hooks.use_state(|| 2_u16);
    let mut system = hooks.use_context_mut::<SystemContext>();
    hooks.use_terminal_events(move |event| {
        if let TerminalEvent::Resize(_, height) = event {
            body_rows.set(height.saturating_sub(5));
        }
        if let TerminalEvent::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            match code {
                KeyCode::Char('q') => closed.set(true),
                KeyCode::Char('z') => acknowledged.set(acknowledged.get() + 1),
                _ => {}
            }
        }
    });
    if closed.get() {
        system.exit();
    }
    let mut choice = SearchSelectState::Closed;
    choice.open(10_u32);
    element! {
        View(width:40,flex_direction:FlexDirection::Column) {
            View(height:u32::from(body_rows.get()),flex_shrink:0.0_f32,flex_direction:FlexDirection::Column) {
                DialogBodyViewport(scroll_keys:DialogScrollKeys::Navigation) {
                    SearchSelect<u32>(state:choice,options:vec![
                        ChoiceOption::available(10,"First"),ChoiceOption::available(20,"Second"),
                        ChoiceOption::available(30,"Third"),ChoiceOption::available(40,"Last")],
                        on_select:move |key|{let mut selected=selected;selected.set(key);})
                }
            }
            Text(content:"FOOTER")
            Text(content:format!("selected:{} ack:{}",selected.get(),acknowledged.get()))
        }
    }
}

#[tokio::test]
async fn clipped_choice_rejects_footer_click_but_accepts_visible_rows_after_scroll() {
    use futures_util::FutureExt;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(ClippedChoiceProbe);
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
    let click = |row| {
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            3,
            row,
        ))
    };
    let journey = async {
        while let Some(mut canvas) = frames.next().await {
            let mut settled = false;
            for _ in 0..64 {
                match frames.next().now_or_never() {
                    Some(Some(next)) => canvas = next,
                    _ => {
                        settled = true;
                        break;
                    }
                }
            }
            assert!(settled, "paint must settle");
            let frame = canvas.to_string();
            match phase {
                0 => {
                    assert!(
                        frame.contains("First") && !frame.contains("Second"),
                        "{frame}"
                    );
                    sender.send(click(2)).unwrap();
                    sender.send(key(KeyCode::Char('z'))).unwrap();
                    phase = 1;
                }
                1 if frame.contains("ack:1") => {
                    assert!(
                        frame.contains("selected:0"),
                        "clipped native option cannot receive footer click: {frame}"
                    );
                    sender.send(click(1)).unwrap();
                    phase = 2;
                }
                2 if frame.contains("selected:10") => {
                    sender.send(key(KeyCode::End)).unwrap();
                    phase = 3;
                }
                3 if frame.contains("Last") => {
                    sender.send(click(1)).unwrap();
                    phase = 4;
                }
                4 if frame.contains("selected:40") => {
                    sender.send(key(KeyCode::Home)).unwrap();
                    sender.send(TerminalEvent::Resize(40, 8)).unwrap();
                    phase = 5;
                }
                5 if frame.contains("Second") && !frame.contains("Third") => {
                    sender.send(click(3)).unwrap();
                    sender.send(key(KeyCode::Char('z'))).unwrap();
                    phase = 6;
                }
                6 if frame.contains("ack:2") => {
                    assert!(
                        frame.contains("selected:40"),
                        "resized clipped Third cannot receive footer click: {frame}"
                    );
                    sender.send(click(2)).unwrap();
                    phase = 7;
                }
                7 if frame.contains("selected:20") => {
                    sender.send(key(KeyCode::Char('q'))).unwrap();
                    phase = 8;
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), journey)
        .await
        .unwrap();
    assert_eq!(phase, 8);
}

#[test]
fn owner_c8_shared_field_values_keep_their_gutter_and_subdued_search_helpers() {
    for stacked in [false, true] {
        let mut positions = Vec::new();
        for selected in [10u32, 20] {
            let mut state = SearchSelectState::Closed;
            state.open(selected);
            let canvas = element!(View(width:60) {
                FormField(label:"Client".to_owned(),label_width:14u16,stacked) {
                    SearchSelect<u32>(state,options:vec![ChoiceOption::available(10,"Local"),ChoiceOption::available(20,"Remote")])
                }
            }).render(None);
            let frame = canvas.to_string();
            assert_eq!(frame.matches("Client").count(), 1, "{frame}");
            let (row, line) = frame
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("Local"))
                .unwrap();
            let column = line
                .get(..line.find("Local").unwrap())
                .unwrap()
                .chars()
                .count();
            positions.push(column);
            assert!(
                column >= if stacked { 2 } else { 16 },
                "value column contains selector gutter: {frame}"
            );
            assert_eq!(
                canvas
                    .cell(column, row)
                    .unwrap()
                    .text_style()
                    .unwrap()
                    .color,
                Some(if selected == 10 {
                    Color::Yellow
                } else {
                    Color::White
                })
            );
            let (row, line) = frame
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("Search:"))
                .unwrap();
            let search_column = line
                .get(..line.find("Search:").unwrap())
                .unwrap()
                .chars()
                .count();
            assert_eq!(
                search_column, column,
                "helper and choice labels align: {frame}"
            );
            assert_eq!(
                canvas
                    .cell(search_column, row)
                    .unwrap()
                    .text_style()
                    .unwrap()
                    .color,
                Some(Color::Grey)
            );
        }
        assert_eq!(
            positions[0], positions[1],
            "focus must not move either value"
        );
    }
}
