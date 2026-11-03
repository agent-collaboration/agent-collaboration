// R14: dropdown Filter replaces inline choice Search.
// R7: shared Help replaces the screen-specific Help title.
use crate::session_picker::new_session::new_session_form::*;
use crate::session_picker::screen::fixtures::fixture_data::picker_request;
use crate::session_picker::screen::picker_component::SessionsPickerComponent;
use futures_util::StreamExt;

struct PendingClientReadDrop {
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    sender: tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
}
impl Drop for PendingClientReadDrop {
    fn drop(&mut self) {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.sender.send(key(KeyCode::F(1)));
    }
}

#[tokio::test]
async fn closing_new_form_cancels_its_read_while_picker_remains_usable() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let loader: crate::session_picker::NewClientInventoryLoader = Arc::new({
        let sender = sender.clone();
        let dropped = dropped.clone();
        move |_| {
            let sender = sender.clone();
            let dropped = dropped.clone();
            Box::pin(async move {
                let _guard = PendingClientReadDrop {
                    count: dropped,
                    sender: sender.clone(),
                };
                sender.send(key(KeyCode::Esc)).unwrap();
                std::future::pending().await
            })
        }
    });
    sender.send(control('n')).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut observed = false;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request:picker_request(),client_inventory_loader:Some(loader),width:76usize,height:30usize,selected_outcome_out:&mut outcome));
        let journey = picker
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .map(|canvas| {
                let frame = canvas.to_string();
                if !observed && frame.contains("Close Help") {
                    assert_eq!(
                        dropped.load(Ordering::SeqCst),
                        1,
                        "closed form must settle its owned read before unrelated input"
                    );
                    observed = true;
                    sender.send(control('d')).unwrap();
                }
                last = frame;
            })
            .collect::<Vec<_>>();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "pending read must cancel while picker stays open: {last}"
        );
    }
    assert!(observed);
    assert!(outcome.is_none());
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control(character: char) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}
fn primary_key() -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

#[tokio::test]
async fn new_form_qualified_cursor_choice_changes_only_the_client_draft() {
    use crate::session_picker::new_session::new_session_client_inventory::CursorCreationAvailability;
    use crate::session_picker::new_session::new_session_client_inventory::NewClientSourceCapabilities;
    use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability;
    let binding:collaboration_client::protocol::ProviderBindingIdentity=serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"actual-cursor"},
        "bindingId":"cursor-binding","runtime":{"provider":"cursor","runtimeName":"fixture"},"transport":"stdioAcp",
        "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":7},
        "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]
    })).unwrap();
    let capabilities = NewClientSourceCapabilities {
        service_id: binding.endpoint.service_id.clone(),
        service_epoch: binding.generation.service_epoch.clone(),
        native_claude: SourceNativeLaunchAvailability::Unavailable {
            message: "Native executable absent".to_owned(),
        },
        cursor: CursorCreationAvailability::Ready(binding),
    };
    let loader: crate::session_picker::NewClientInventoryLoader =
        std::sync::Arc::new(move |source| {
            assert_eq!(source, PickerSourceContext::DefaultHosted);
            let capabilities = capabilities.clone();
            Box::pin(async move { Ok(capabilities) })
        });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request:picker_request(),client_inventory_loader:Some(loader),width:120usize,height:36usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(control('n')).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("New session") => {
                        sender.send(key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Filter:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("Cursor".to_owned()))
                            .unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("Filter: Cursor") => {
                        sender.send(key(KeyCode::Enter)).unwrap();
                        phase = 4;
                    }
                    4 if !frame.contains("Filter:")
                        && frame.lines().any(|line| {
                            line.contains("Client") && line.contains("Cursor") && line.contains('▾')
                        }) =>
                    {
                        assert!(
                            frame.contains("/repo/project-a"),
                            "client selection must retain directory: {frame}"
                        );
                        sender.send(control('d')).unwrap();
                        phase = 5;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 5);
    assert!(
        outcome.is_none(),
        "qualifying and selecting a client must not submit it"
    );
}

#[tokio::test]
async fn new_form_constrained_focus_and_resize_retain_query_and_directory_without_start() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request:picker_request(),width:0usize,height:0usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let text = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(TerminalEvent::Resize(48, 12)).unwrap();
                        sender.send(control('n')).unwrap();
                        phase = 1;
                    }
                    1 if text.contains("New session")
                        && canvas.width() == 48
                        && canvas.height() == 12 =>
                    {
                        assert!(text.contains("Start") && text.contains("Cancel"), "{text}");
                        for event in [key(KeyCode::Tab), key(KeyCode::Enter)] {
                            sender.send(event).unwrap();
                        }
                        phase = 2;
                    }
                    2 if text.contains("Filter:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("this".to_owned()))
                            .unwrap();
                        phase = 3;
                    }
                    3 if text.contains("Filter: this") => {
                        sender.send(TerminalEvent::Resize(76, 30)).unwrap();
                        phase = 4;
                    }
                    4 if canvas.width() == 76
                        && canvas.height() == 30
                        && text.contains("Filter: this") =>
                    {
                        sender.send(key(KeyCode::Esc)).unwrap();
                        sender.send(key(KeyCode::Tab)).unwrap();
                        sender.send(TerminalEvent::Resize(48, 12)).unwrap();
                        phase = 5;
                    }
                    5 if canvas.width() == 48
                        && canvas.height() == 12
                        && text.contains("Directory")
                        && text.contains("/repo/project-a") =>
                    {
                        assert!(
                            text.contains("New session")
                                && text.contains("Start")
                                && text.contains("Cancel"),
                            "{text}"
                        );
                        sender.send(control('d')).unwrap();
                        phase = 6;
                    }
                    _ => {}
                }
                last = text;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), journey).await;
        assert!(result.is_ok(), "phase={phase}: {last}");
    }
    assert_eq!(phase, 6, "{last}");
    assert!(outcome.is_none(), "focus/resize cannot start a session");
}

#[tokio::test]
async fn new_form_shows_supplied_arguments_before_explicit_start() {
    let mut request = picker_request();
    request.native_arguments = [
        "--model",
        "fixture-model",
        "--config",
        "policy=literal space",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    request.new_session_args_display =
        "--model fixture-model --config 'policy=literal space'".to_owned();
    let mut model = SessionsPickerModel::new(request.clone(), 120);
    model.open_new_form();
    assert_eq!(
        model
            .new_session
            .as_ref()
            .expect("opened New draft")
            .arguments,
        request.native_arguments,
        "opening the form must preserve the actual invocation tokens"
    );
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request,width:120usize,height:36usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(control('n')).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("New session") && frame.contains("Arguments") => {
                        assert!(frame.contains("fixture-model"), "{frame}");
                        assert!(frame.contains("policy=literal space"), "{frame}");
                        assert!(
                            frame.contains("Start") && frame.contains("Cancel"),
                            "{frame}"
                        );
                        sender.send(primary_key()).unwrap();
                        phase = 2;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 2, "{last}");
    assert_eq!(outcome, Some(SessionsPickerOutcome::StartNewSession));
}

#[tokio::test]
async fn new_form_client_choice_search_cancel_retains_one_draft_before_start() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    {
        let mut picker = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(control('n')).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("New session") => {
                        assert!(
                            frame.lines().any(|line| line.contains("Client")
                                && line.contains("Codex")
                                && line.contains('▾')),
                            "New must expose the selected client before Start:\n{frame}"
                        );
                        sender.send(key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Filter:") => {
                        sender
                            .send(TerminalEvent::BracketedPaste("cUrSoR".to_owned()))
                            .unwrap();
                        phase = 3;
                    }
                    3 if frame.contains("Filter: cUrSoR") => {
                        assert!(frame.contains("Cursor"), "{frame}");
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 4;
                    }
                    4 if frame.contains("New session") && !frame.contains("Filter:") => {
                        assert!(
                            frame.contains("Codex") && frame.contains("/repo/project-a"),
                            "cancel choice must retain draft fields: {frame}"
                        );
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 5;
                    }
                    5 if !frame.contains("New session") => {
                        sender.send(control('d')).unwrap();
                        phase = 6;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), journey)
                .await
                .is_ok(),
            "phase={phase}: {last}"
        );
    }
    assert_eq!(phase, 6, "{last}");
    assert!(
        outcome.is_none(),
        "client search/cancellation cannot submit a session"
    );
}
