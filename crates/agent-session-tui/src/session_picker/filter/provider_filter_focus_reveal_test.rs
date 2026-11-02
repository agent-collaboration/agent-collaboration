//! R14: dropdowns replace inline choice/action rows; refusal and draft identity remain.
use crate::session_picker::filter::provider_filter_model::SessionProviderFilter;
use crate::session_picker::filter::provider_filter_test::mixed_request;
use crate::session_picker::test_support::*;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::OverlayKind;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn compact_text(value: &str) -> String {
    value
        .chars()
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
    let observed = Arc::new(Mutex::new(
        None::<(SurfaceStackHandle, State<SessionsPickerModel>)>,
    ));
    let sink = observed.clone();
    let processed = Arc::new(AtomicUsize::new(0));
    let processed_sink = processed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut picker = element!(SessionsPickerComponent(request:mixed_request(),width,height,selected_outcome_out:&mut outcome,
        surface_observer:move |value| *sink.lock().unwrap()=Some(value),
        input_observer:move |()| {processed_sink.fetch_add(1,Ordering::SeqCst);}));
    let mut phase = 0;
    let mut expected_processed = 0;
    let mut last = String::new();
    let mut send = |events: Vec<TerminalEvent>| {
        expected_processed += events.len();
        for event in events {
            sender.send(event).unwrap();
        }
        expected_processed
    };
    let mut barrier = 0;
    let result = {
        let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let journey = async {
            while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
                if phase == 10 {
                    continue;
                }
                let frame = canvas.to_string();
                let (stack, model) = observed.lock().unwrap().as_ref().cloned().unwrap();
                let snapshot = stack.snapshot();
                if processed.load(Ordering::SeqCst) < barrier {
                    continue;
                }
                let active_field =
                    snapshot
                        .overlays()
                        .last()
                        .and_then(|overlay| match &overlay.kind {
                            OverlayKind::Dropdown { field } => Some(field.as_str()),
                            _ => None,
                        });
                let dropdown = active_field
                    .map(|_| dialog_text(&frame, dialog_bounds(&frame, "Filter:")))
                    .unwrap_or_default();
                let compact = compact_text(&dropdown);
                match phase {
                    0 => {
                        barrier = send(vec![
                            ctrl_key('f'),
                            key(KeyCode::Enter),
                            TerminalEvent::BracketedPaste("Claude".into()),
                        ]);
                        phase = 1;
                    }
                    1 if compact.contains("Filter:Claude") => {
                        barrier = send(vec![key(KeyCode::Enter), resume_key()]);
                        phase = 2;
                    }
                    2 if snapshot.overlays().is_empty() => {
                        assert_eq!(
                            model.read().provider_filter,
                            SessionProviderFilter::ClaudeCode
                        );
                        barrier = send(vec![
                            ctrl_key('f'),
                            key(KeyCode::Tab),
                            key(KeyCode::Enter),
                            TerminalEvent::BracketedPaste("Checkout".into()),
                            key(KeyCode::Enter),
                        ]);
                        phase = 3;
                    }
                    3 if compact.contains("Filter:Checkout") => {
                        assert_eq!(
                            active_field,
                            Some("filter-folder"),
                            "query belongs to Folder"
                        );
                        assert!(
                            dropdown_reason_text(&dropdown, "Checkout scope")
                                .contains("CheckoutscopeissupportedbyCLI--checkout--listonly"),
                            "full refusal: {frame}"
                        );
                        assert_eq!(
                            model
                                .read()
                                .provider_filter_draft
                                .as_ref()
                                .unwrap()
                                .query
                                .root,
                            SessionsRoot::Any,
                            "unavailable Checkout cannot change the draft"
                        );
                        barrier = send(vec![
                            key(KeyCode::Esc),
                            key(KeyCode::Tab),
                            key(KeyCode::Enter),
                        ]);
                        phase = 4;
                    }
                    4 if active_field == Some("filter-model-provider") => {
                        assert!(
                            compact.contains("Anymodelprovider"),
                            "selected provider remains visible: {frame}"
                        );
                        barrier = send(vec![
                            key(KeyCode::Esc),
                            key(KeyCode::Tab),
                            key(KeyCode::Enter),
                            TerminalEvent::BracketedPaste("Interactive".into()),
                            key(KeyCode::Enter),
                        ]);
                        phase = 5;
                    }
                    5 if compact.contains("Filter:Interactive") => {
                        assert_eq!(
                            active_field,
                            Some("filter-source"),
                            "query belongs to Source"
                        );
                        assert!(
                            dropdown_reason_text(&dropdown, "External provider")
                                .contains("ExternalproviderinventoriesrequireAllsources"),
                            "full source refusal: {frame}"
                        );
                        assert_eq!(
                            model
                                .read()
                                .provider_filter_draft
                                .as_ref()
                                .unwrap()
                                .query
                                .source,
                            SessionsSource::All
                        );
                        barrier = send(vec![
                            key(KeyCode::Esc),
                            key(KeyCode::Enter),
                            TerminalEvent::BracketedPaste("Subagents".into()),
                            key(KeyCode::Enter),
                        ]);
                        phase = 6;
                    }
                    6 if compact.contains("Filter:Subagents") => {
                        assert!(
                            dropdown_reason_text(&dropdown, "External provider")
                                .contains("ExternalproviderinventoriesrequireAllsources"),
                            "full subagent refusal: {frame}"
                        );
                        assert_eq!(
                            model
                                .read()
                                .provider_filter_draft
                                .as_ref()
                                .unwrap()
                                .query
                                .source,
                            SessionsSource::All
                        );
                        barrier = send(vec![
                            key(KeyCode::Esc),
                            key(KeyCode::Enter),
                            TerminalEvent::BracketedPaste("All".into()),
                        ]);
                        phase = 7;
                    }
                    7 if compact.contains("Filter:All") => {
                        assert!(compact.contains("Allsources"));
                        assert!(
                            !compact.contains("Interactive") && !compact.contains("Subagents"),
                            "query removes nonmatching choices"
                        );
                        barrier = send(vec![key(KeyCode::Esc)]);
                        phase = 8;
                    }
                    8 if active_field.is_none() => {
                        assert_eq!(
                            model
                                .read()
                                .provider_filter_draft
                                .as_ref()
                                .unwrap()
                                .query
                                .source,
                            SessionsSource::All
                        );
                        barrier = send(vec![key(KeyCode::Esc)]);
                        phase = 9;
                    }
                    9 if snapshot.overlays().is_empty() => {
                        assert_eq!(
                            model.read().provider_filter,
                            SessionProviderFilter::ClaudeCode,
                            "choice cancellation preserves the applied client"
                        );
                        send(vec![ctrl_key('d')]);
                        phase = 10;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        tokio::time::timeout(Duration::from_secs(4), journey).await
    };
    drop(picker);
    assert!(result.is_ok(), "source witness phase{phase}: {last}");
    assert_eq!(phase, 10, "source witness did not complete: {last}");
    assert!(
        outcome.is_none(),
        "choice navigation cannot launch a session"
    );
}
