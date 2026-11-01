//! Generated Help projects real bindings and retains shipped section styles.
use super::*;

// R7/R13: click Menu then Help; the generated rows must match the screen's real bindings.
#[tokio::test]
async fn sessions_help_click_opens_overlay_with_surface_and_section_styles() {
    use agent_collaboration_tui::surfaces::{
        surface_handle::SurfaceStackHandle, surface_stack::SurfaceId,
    };
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let sink = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,surface_observer:move |(stack,_)|*sink.lock().unwrap()=Some(stack)));
    let mut phase = 0;
    let mut expected = Vec::<String>::new();
    let mut seen = std::collections::HashSet::new();
    let mut section_style_checked = false;
    let mut last = String::new();
    let click = |frame: &str, label: &str| {
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .expect("drawn action");
        let column =
            unicode_width::UnicodeWidthStr::width(line.get(..line.find(label).unwrap()).unwrap());
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            u16::try_from(column).unwrap(),
            u16::try_from(row).unwrap(),
        ))
    };
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| {
            if phase == 3 {
                return;
            }
            let frame = canvas.to_string();
            let stack = observed.lock().unwrap().as_ref().copied().unwrap();
            match phase {
                0 => {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains(" sessions ·"))
                        .unwrap();
                    let column = unicode_width::UnicodeWidthStr::width(
                        line.split_once("sessions").unwrap().0,
                    );
                    let style = canvas.cell(column, row).unwrap().text_style().unwrap();
                    assert_eq!(style.color, Some(Color::Cyan));
                    assert_eq!(style.weight, Weight::Bold);
                    expected = stack
                        .snapshot()
                        .surface(SurfaceId::Screen)
                        .unwrap()
                        .actions
                        .presentations()
                        .into_iter()
                        .map(|action| {
                            format!(
                                "{}{}",
                                if action.keys.is_empty() {
                                    String::new()
                                } else {
                                    action
                                        .keys
                                        .iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                },
                                action.title
                            )
                            .chars()
                            .filter(|character| !character.is_whitespace())
                            .collect()
                        })
                        .collect();
                    sender.send(click(&frame, "Menu")).unwrap();
                    phase = 1;
                }
                1 if frame.contains("Help") => {
                    sender.send(click(&frame, "Help")).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Close Help") => {
                    if let Some((row, line)) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Actions"))
                    {
                        let column = unicode_width::UnicodeWidthStr::width(
                            line.split_once("Actions").unwrap().0,
                        );
                        let section_style = canvas.cell(column, row).unwrap().text_style().unwrap();
                        assert_eq!(
                            section_style.color,
                            Some(Color::Rgb {
                                r: 156,
                                g: 188,
                                b: 251
                            })
                        );
                        assert_eq!(section_style.weight, Weight::Bold);
                        section_style_checked = true;
                    }
                    for line in frame.lines() {
                        let compact = line
                            .chars()
                            .filter(|character| !character.is_whitespace())
                            .collect::<String>();
                        for pair in &expected {
                            if compact.contains(pair) {
                                seen.insert(pair.clone());
                            }
                        }
                    }
                    if expected.iter().all(|pair| seen.contains(pair)) {
                        sender.send(ctrl_key('d')).unwrap();
                        phase = 3;
                    } else {
                        sender.send(key(KeyCode::PageDown)).unwrap();
                    }
                }
                _ => {}
            }
            last = frame;
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("click Help and read every actual binding");
    assert!(
        section_style_checked,
        "the generated Actions section was not observed"
    );
    assert_eq!(
        phase,
        3,
        "missing generated bindings: {:?}: {last}",
        expected
            .into_iter()
            .filter(|pair| !seen.contains(pair))
            .collect::<Vec<_>>()
    );
}
