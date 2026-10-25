//! A focused expanded field must remain attributable after the body settles.
use super::*;
use crate::presentation::session_picker::provider_filter_tests::mixed_request;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

fn is_source_label(line: &str) -> bool {
    line.trim_matches(|character: char| character.is_whitespace() || matches!(character, '│' | '┃'))
        == "Source"
}

#[tokio::test]
async fn source_choice_48x24_keeps_field_label_with_selected_option_in_settled_frame() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker =
        element!(SessionsPickerComponent(request:mixed_request(),width:48usize,height:24usize));
    let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut last = String::new();
    let send = |event| {
        sender
            .send(event)
            .expect("mounted witness input remains connected")
    };
    let journey = async {
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            let body = if frame.contains("Session filters") {
                dialog_text(&frame, dialog_bounds(&frame, "Session filters"))
            } else {
                String::new()
            };
            match phase {
                0 if frame.contains("Sessions") => {
                    send(ctrl_key('f'));
                    phase = 1;
                }
                1 if body.contains("Client") && !body.contains("Search:") => {
                    send(key(KeyCode::Enter));
                    phase = 2;
                }
                2 if body.contains("Search:") => {
                    send(TerminalEvent::BracketedPaste("Claude".to_owned()));
                    phase = 3;
                }
                3 if body.contains("Search: Claude") => {
                    send(key(KeyCode::Enter));
                    phase = 4;
                }
                4 if body.contains("Claude Code ▾") && !body.contains("Search:") => {
                    for _ in 0..6 {
                        send(key(KeyCode::Tab));
                    }
                    send(key(KeyCode::Enter));
                    phase = 5;
                }
                5 if !frame.contains("Session filters") && frame.contains("Claude Code") => {
                    send(ctrl_key('f'));
                    phase = 6;
                }
                6 if body.contains("Client") && !body.contains("Search:") => {
                    send(key(KeyCode::Tab));
                    send(key(KeyCode::Enter));
                    phase = 7;
                }
                7 if body.contains("Search:") => {
                    send(TerminalEvent::BracketedPaste("Checkout".to_owned()));
                    // Checkout is unavailable. Enter must leave its choice open.
                    send(key(KeyCode::Enter));
                    phase = 8;
                }
                8 if body.contains("Search: Checkout") => {
                    send(key(KeyCode::Esc));
                    send(key(KeyCode::Tab));
                    send(key(KeyCode::Enter));
                    phase = 9;
                }
                9 if body.contains("Search:") && body.contains("Any model provider") => {
                    send(key(KeyCode::Esc));
                    send(key(KeyCode::Tab));
                    phase = 10;
                }
                10 if body.contains("All sources ▾") && !body.contains("Search:") => {
                    assert!(
                        body.lines().any(is_source_label),
                        "closed Source field must be visible before expansion: {frame}"
                    );
                    send(key(KeyCode::Enter));
                    phase = 11;
                }
                11 if body
                    .lines()
                    .any(|line| line.contains('❯') && line.contains("All sources")) =>
                {
                    println!("SOURCE48x24 SETTLED ORIGINAL:\n{frame}");
                    let lines = body.lines().collect::<Vec<_>>();
                    let source = lines.iter().position(|line| is_source_label(line));
                    let search = lines.iter().position(|line| line.contains("Search:"));
                    let selected = lines
                        .iter()
                        .position(|line| line.contains('❯') && line.contains("All sources"));
                    assert!(
                        source.is_some(),
                        "expanded Source label disappeared while its selected option stayed visible: {frame}"
                    );
                    assert!(
                        search.is_some(),
                        "expanded Source query disappeared: {frame}"
                    );
                    assert!(
                        source < search && search < selected,
                        "field/query/selection must remain associated in this same dialog: {frame}"
                    );
                    assert_eq!(
                        body.matches("External provider inventories").count(),
                        2,
                        "{frame}"
                    );
                    assert!(
                        body.contains("Interactive") && body.contains("Subagents"),
                        "{frame}"
                    );
                    assert!(
                        body.lines()
                            .any(|line| line.contains("Cancel") && line.contains("Select")),
                        "choice actions must remain fixed and reachable: {frame}"
                    );
                    send(key(KeyCode::Esc));
                    phase = 12;
                }
                12 if !body.contains("Search:") && body.contains("All sources ▾") => {
                    assert!(body.contains("Apply") && body.contains("Cancel"), "{frame}");
                    send(key(KeyCode::Esc));
                    phase = 13;
                }
                13 if !frame.contains("Session filters") => {
                    assert!(
                        frame.contains("Claude Code"),
                        "choice cancel changed the applied client: {frame}"
                    );
                    send(ctrl_key('d'));
                    phase = 14;
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(4), journey).await;
    assert!(result.is_ok(), "source witness phase={phase}: {last}");
    assert_eq!(phase, 14, "source witness did not complete: {last}");
}
