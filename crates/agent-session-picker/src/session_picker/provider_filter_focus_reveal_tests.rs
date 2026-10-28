//! A focused expanded field must remain attributable after the body settles.
use super::*;
use crate::session_picker::provider_filter_tests::mixed_request;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

fn is_field_label(line: &str, label: &str) -> bool {
    line.trim_matches(|character: char| character.is_whitespace() || matches!(character, '│' | '┃'))
        == label
}

fn rendered_text_without_spacing(body: &str) -> String {
    body.chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '│' | '┃'))
        .collect()
}

#[tokio::test]
async fn source_choice_48x24_keeps_field_label_with_selected_option_in_settled_frame() {
    assert_source_choice_frame(48, 24).await;
}

#[tokio::test]
async fn source_choice_48x12_keeps_query_reasons_and_selection_in_settled_frame() {
    assert_source_choice_frame(48, 12).await;
}

#[tokio::test]
async fn filter_choices_32x24_keep_labels_queries_and_full_reasons() {
    assert_source_choice_frame(32, 24).await;
}

#[tokio::test]
async fn filter_choices_40x24_keep_labels_queries_and_full_reasons() {
    assert_source_choice_frame(40, 24).await;
}

async fn assert_source_choice_frame(width: usize, height: usize) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker = element!(SessionsPickerComponent(request:mixed_request(),width,height));
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
                    let lines = body.lines().collect::<Vec<_>>();
                    let folder = lines.iter().position(|line| is_field_label(line, "Folder"));
                    let search = lines.iter().position(|line| line.contains("Search:"));
                    let selected = lines
                        .iter()
                        .position(|line| line.contains('❯') && line.contains("Any folder"));
                    assert!(
                        folder.is_some() && search.is_some() && selected.is_some(),
                        "{frame}"
                    );
                    assert!(folder < search && search < selected, "{frame}");
                    let normalized = rendered_text_without_spacing(&body);
                    assert!(body.contains("Current directory"), "{frame}");
                    assert!(normalized.contains("Repository/linkedworktrees"), "{frame}");
                    if width >= 48 {
                        assert!(body.contains("Repository / linked worktrees"), "{frame}");
                    }
                    // Wrapping may split the literal --checkout token; require
                    // every non-space character in order across rendered rows.
                    assert!(
                        normalized.contains("CheckoutscopeissupportedbyCLI--checkout--listonly"),
                        "{frame}"
                    );
                    assert!(
                        body.contains("Cancel") && body.contains("Select"),
                        "{frame}"
                    );
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
                9 if body.contains("Search:")
                    && rendered_text_without_spacing(&body).contains("❯Anymodelprovider") =>
                {
                    send(key(KeyCode::Esc));
                    send(key(KeyCode::Tab));
                    phase = 10;
                }
                10 if body.contains("All sources ▾") && !body.contains("Search:") => {
                    assert!(
                        body.lines().any(|line| is_field_label(line, "Source")),
                        "closed Source field must be visible before expansion: {frame}"
                    );
                    send(key(KeyCode::Enter));
                    phase = 11;
                }
                11 if body
                    .lines()
                    .any(|line| line.contains('❯') && line.contains("All sources")) =>
                {
                    println!("SOURCE{width}x{height} SETTLED ORIGINAL:\n{frame}");
                    let lines = body.lines().collect::<Vec<_>>();
                    let source = lines.iter().position(|line| {
                        is_field_label(line, "Source")
                            || (height < 16 && line.contains("Source") && line.contains("Search:"))
                    });
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
                        (source < search || (height < 16 && source == search)) && search < selected,
                        "field/query/selection must remain associated in this same dialog: {frame}"
                    );
                    if source == search {
                        let line = lines[source.unwrap()];
                        assert!(line.find("Source") < line.find("Search:"), "{frame}");
                    }
                    let normalized = rendered_text_without_spacing(&body);
                    assert!(normalized.contains("InteractiveExternalproviderinventoriesrequireAllsourcesSubagentsExternalproviderinventoriesrequireAllsources❯Allsources"), "{frame}");
                    if width >= 48 {
                        assert_eq!(
                            normalized
                                .matches("ExternalproviderinventoriesrequireAllsources")
                                .count(),
                            2,
                            "{frame}"
                        );
                    }
                    assert!(
                        body.contains("Interactive") && body.contains("Subagents"),
                        "{frame}"
                    );
                    assert!(
                        body.lines()
                            .any(|line| line.contains("Cancel") && line.contains("Select")),
                        "choice actions must remain fixed and reachable: {frame}"
                    );
                    send(TerminalEvent::BracketedPaste("All".to_owned()));
                    phase = 15;
                }
                15 if body.contains("Search: All") => {
                    assert!(
                        body.contains("Source") && body.contains("All sources"),
                        "{frame}"
                    );
                    assert!(
                        !body.contains("Interactive") && !body.contains("Subagents"),
                        "{frame}"
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
