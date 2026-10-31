//! S6 consumer obligations, with expectations from R4, R9 and R13.
use super::*;

#[tokio::test]
async fn details_copy_status_stays_below_the_scrolling_body() {
    use agent_collaboration_tui::{hit_regions::HitRegionKind, surface_handle::SurfaceStackHandle};

    for (width, height, failure) in [(76usize, 30usize, false), (48, 24, true), (32, 24, false)] {
        let mut request = picker_request();
        request.records[0].conversation.snippets = (0..40)
            .map(|index| format!("PINNED_BODY {index}"))
            .collect();
        let copier: crate::session_picker::session_id_clipboard::SessionClipboardWriter =
            Arc::new(move |_| {
                if failure {
                    Err(std::io::ErrorKind::BrokenPipe.into())
                } else {
                    Ok(())
                }
            });
        let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
        let sink = observed.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(key(KeyCode::F(4))).unwrap();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(SessionsPickerComponent(request,width,height,
            session_clipboard_writer:Some(copier),
            surface_observer:move |(stack,_)|*sink.lock().unwrap()=Some(stack)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let expected = if failure {
            "Could not copy"
        } else {
            "Copy requested"
        };
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Copy session ID") {
                    for code in [KeyCode::Tab, KeyCode::Tab, KeyCode::Enter] {
                        sender.send(key(code)).unwrap();
                    }
                    phase = 1;
                } else if phase == 1 && frame.contains(expected) {
                    let stack = observed.lock().unwrap().as_ref().copied().unwrap();
                    let map = stack.hit_regions().published().unwrap();
                    let body_bottom = map
                        .targets
                        .iter()
                        .find(|region| {
                            region.surface == stack.snapshot().top_id()
                                && region.kind == HitRegionKind::ScrollArea
                        })
                        .unwrap()
                        .cells
                        .iter()
                        .map(|(_, row)| usize::from(*row))
                        .max()
                        .unwrap();
                    let status_row = frame
                        .lines()
                        .position(|line| line.contains(expected))
                        .unwrap();
                    assert!(
                        status_row > body_bottom,
                        "action feedback must be outside the scrolling body: {frame}"
                    );
                    assert_eq!(
                        frame.matches(expected).count(),
                        1,
                        "one current status: {frame}"
                    );
                    sender.send(key(KeyCode::End)).unwrap();
                    phase = 2;
                } else if phase == 2 && frame.contains("PINNED_BODY 39") {
                    assert!(
                        frame.contains(expected),
                        "scrolling must retain status: {frame}"
                    );
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 3;
                }
                last = frame;
            }
        };
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .unwrap_or_else(|_| panic!("copy status {width}x{height},phase{phase}: {last}"));
        assert_eq!(phase, 3);
    }
}

#[tokio::test]
async fn unreachable_source_hint_requires_a_live_record_loader() {
    use agent_collaboration_tui::surface_handle::SurfaceStackHandle;
    use agent_session_inventory::source_inventory_request::{
        PickerSourceContext, SourceInventoryRejection, SourceInventoryResult,
    };
    use std::sync::atomic::AtomicBool;

    for has_loader in [true, false] {
        let loader: SessionsPickerRecordLoader = Arc::new(|request| {
            Box::pin(async move {
                SourceInventoryResult::Rejected {
                    request,
                    reason: SourceInventoryRejection::SourceUnavailable,
                }
            })
        });
        let record_loader = has_loader.then_some(loader);
        let configured = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut ui = element!(SessionsPickerComponent(request:picker_request(),record_loader,width:120usize,height:36usize,
        surface_observer:move |(_,mut model):(SurfaceStackHandle,State<SessionsPickerModel>)| {
            if configured.swap(true,Ordering::SeqCst) || has_loader {return;}
            model.write().source_progress=vec![crate::session_picker::source_reload_progress::SourceReadProgress {
                source_context:PickerSourceContext::DefaultHosted,
                read_state:crate::session_picker::source_reload_progress::SourceReadState::Rejected {reason:SourceInventoryRejection::SourceUnavailable},
            }];
        }));
        let frames = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut captured = None;
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let text = canvas.to_string();
                if captured.is_none() && text.contains("not reachable") {
                    captured = Some(text);
                    sender.send(ctrl_key('d')).unwrap();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .expect("the real source failure and command projection must render");
        let text = captured.expect("the unreachable source notice was reached");
        assert_eq!(
            text.contains("Retry: Ctrl+R Refresh"),
            has_loader,
            "a retry hint requires a real bound loader: {text}"
        );
    }
}

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[tokio::test]
async fn escape_at_defaults_keeps_picker_alive_for_help() {
    // Owner: "esc doesn't quit, ctrl c does" (R4).
    let frame = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![key(KeyCode::Esc), key(KeyCode::F(1)), ctrl_key('d')],
    )
    .await;
    assert!(
        frame.contains("Help"),
        "Esc prevented the subsequent Help action: {frame}"
    );
    assert!(
        frame.contains("Close"),
        "Help must still open after Esc: {frame}"
    );
}

#[tokio::test]
async fn toolbar_exposes_menu_new_filter_and_folder_scope() {
    let frame = render_picker_capture_at(picker_request(), 120, 36, vec![ctrl_key('d')]).await;
    for label in ["Menu", "F10", "+ New", "Filter", "This folder", "All"] {
        assert!(
            frame.contains(label),
            "R13 toolbar is missing {label}: {frame}"
        );
    }
}

#[tokio::test]
async fn menu_is_a_surface_and_never_lists_itself_or_input_conventions() {
    let (canvases, bounds) = render_anchored_picker_canvases(
        picker_request(),
        120,
        36,
        vec![key(KeyCode::F(10))],
        None,
        agent_collaboration_tui::surface_stack::OverlayKind::Menu,
    )
    .await;
    let frame = dialog_text(&canvases.last().unwrap().to_string(), bounds);
    for label in [
        "New session",
        "Details",
        "Open",
        "Fork",
        "Refresh",
        "Help",
        "Quit",
    ] {
        assert!(
            frame.contains(label),
            "R13 menu is missing {label}: {frame}"
        );
    }
    assert_eq!(
        frame.matches("Menu").count(),
        0,
        "menu must not list itself: {frame}"
    );
    assert!(
        !frame.contains("Move field focus"),
        "input conventions belong in Help: {frame}"
    );
}

#[tokio::test]
async fn new_dialog_down_moves_to_machine_instead_of_starting() {
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let (canvases, _) = render_anchored_picker_canvases(
        picker_request(),
        120,
        36,
        vec![ctrl_key('n'), key(KeyCode::Down), key(KeyCode::Enter)],
        Some(&mut outcome),
        agent_collaboration_tui::surface_stack::OverlayKind::Dropdown {
            field: agent_collaboration_tui::focus_model::TargetId::new("new-machine").unwrap(),
        },
    )
    .await;
    let frames = canvases.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(
        outcome.is_none(),
        "Down then Enter must open Machine, never Start: {outcome:?}"
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("New session") && frame.contains("Filter:")),
        "Machine opens its dropdown after Down then Enter: {frames:?}"
    );
}

#[tokio::test]
async fn filter_focus_reaches_apply_cancel_and_back_through_input_owner() {
    use agent_collaboration_tui::{
        actions::ActionId, surface_handle::SurfaceStackHandle, surface_stack::SurfaceId,
    };
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let sink = observed.clone();
    let processed = Arc::new(AtomicUsize::new(0));
    let processed_sink = processed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(ctrl_key('f')).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,
        surface_observer:move |(stack, _)| *sink.lock().unwrap()=Some(stack),
        input_observer:move |()| { processed_sink.fetch_add(1, Ordering::SeqCst); }));
    let mut phase = 0;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|_| {
            if phase == 4 {
                return;
            }
            let stack = observed.lock().unwrap().as_ref().copied().unwrap();
            let snapshot = stack.snapshot();
            if snapshot.overlays().is_empty() {
                return;
            }
            match phase {
                0 => {
                    for _ in 0..6 {
                        sender.send(key(KeyCode::Tab)).unwrap();
                    }
                    phase = 1;
                }
                1 if processed.load(Ordering::SeqCst) >= 7 => {
                    assert_eq!(
                        snapshot.top().focus.current().unwrap().as_str(),
                        "action:filters-apply"
                    );
                    sender.send(key(KeyCode::Tab)).unwrap();
                    phase = 2;
                }
                2 if processed.load(Ordering::SeqCst) >= 8 => {
                    assert_eq!(
                        snapshot.top().focus.current().unwrap().as_str(),
                        "action:dialog-close"
                    );
                    sender.send(key(KeyCode::BackTab)).unwrap();
                    phase = 3;
                }
                3 if processed.load(Ordering::SeqCst) >= 9 => {
                    assert_eq!(
                        snapshot.top().focus.current().unwrap().as_str(),
                        "action:filters-apply"
                    );
                    snapshot
                        .surface(SurfaceId::Screen)
                        .unwrap()
                        .actions
                        .invocation(&ActionId::new("quit").unwrap())
                        .unwrap()(());
                    phase = 4;
                }
                _ => {}
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("real filter focus traversal");
    assert_eq!(phase, 4);
}

#[tokio::test]
async fn an_accepted_start_outcome_cannot_be_replaced_by_a_following_open_callback() {
    use agent_collaboration_tui::{
        actions::ActionId, surface_handle::SurfaceStackHandle, surface_stack::SurfaceId,
    };
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let sink = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(ctrl_key('n')).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,selected_outcome_out:&mut outcome,surface_observer:move |(stack,_)| *sink.lock().unwrap()=Some(stack)));
    let mut invoked = false;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|_| {
            if invoked {
                return;
            }
            let stack = observed.lock().unwrap().as_ref().copied().unwrap();
            let snapshot = stack.snapshot();
            if snapshot.overlays().len() != 1 {
                return;
            }
            let start = snapshot
                .top()
                .actions
                .invocation(&ActionId::new("new-start").unwrap())
                .unwrap();
            let open = snapshot
                .surface(SurfaceId::Screen)
                .unwrap()
                .actions
                .invocation(&ActionId::new("open").unwrap())
                .unwrap();
            let quit = snapshot
                .surface(SurfaceId::Screen)
                .unwrap()
                .actions
                .invocation(&ActionId::new("quit").unwrap())
                .unwrap();
            start(());
            open(());
            quit(());
            invoked = true;
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("accepted primary outcome must end the real picker");
    drop(component);
    assert!(invoked);
    assert_eq!(
        outcome,
        Some(SessionsPickerOutcome::StartNewSession),
        "later callbacks cannot retarget an accepted Start"
    );
}

#[cfg(feature = "quota-reset-test-harness")]
#[tokio::test]
async fn returned_hosted_codex_identity_keeps_service_endpoint_and_same_opaque_id() {
    use crate::session_picker::provider_filter_storyboard_fixture::{
        apply_storyboard_variant, provider_filter_storyboard_request,
    };
    use agent_session_inventory::sessions::SessionPickerIdentity;
    for (query, service, endpoint) in [
        (
            "Quartz",
            "00000000-0000-4000-8000-000000000001",
            "codex-source-a",
        ),
        (
            "Cedar",
            "00000000-0000-4000-8000-000000000002",
            "codex-source-a",
        ),
        (
            "Indigo",
            "00000000-0000-4000-8000-000000000002",
            "codex-source-b",
        ),
    ] {
        let request = apply_storyboard_variant(
            provider_filter_storyboard_request().unwrap(),
            Some("source-qualified-outcome"),
        )
        .unwrap();
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let mut component = element!(SessionsPickerComponent(request,width:120usize,height:36usize,selected_outcome_out:&mut outcome));
        let events = futures_util::stream::iter([
            key(KeyCode::Char('/')),
            TerminalEvent::BracketedPaste(query.into()),
            resume_key(),
        ]);
        tokio::time::timeout(
            Duration::from_secs(2),
            component
                .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
                .collect::<Vec<_>>(),
        )
        .await
        .expect("filter then Open must return the selected source");
        drop(component);
        let Some(SessionsPickerOutcome::ResumeSession(selection)) = outcome else {
            panic!("actual Open outcome for {query}");
        };
        let expected=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":service,"endpointId":endpoint},"sessionId":"00000000-0000-4000-8000-000000000011"})).unwrap();
        assert_eq!(
            selection.identity,
            SessionPickerIdentity::HostedCodex(expected),
            "same-ID rows cannot collapse their source qualification"
        );
    }
}

#[tokio::test]
async fn closing_help_restores_the_content_height_of_the_new_form() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component =
        element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize));
    let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut original_height = None;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            match phase {
                0 => {
                    sender.send(ctrl_key('n')).unwrap();
                    phase = 1;
                }
                1 if frame.contains("New session") => {
                    let bounds = dialog_bounds(&frame, "New session");
                    original_height = Some(bounds.bottom - bounds.top);
                    sender.send(key(KeyCode::F(1))).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Close Help") => {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 3;
                }
                3 if frame.contains("New session") && !frame.contains("Close Help") => {
                    let bounds = dialog_bounds(&frame, "New session");
                    if Some(bounds.bottom - bounds.top) == original_height {
                        sender.send(ctrl_key('d')).unwrap();
                        phase = 4;
                    }
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
    assert!(
        result.is_ok(),
        "R7/R11: Help must restore the original content height: {last}"
    );
    assert_eq!(phase, 4, "{last}");
}

#[tokio::test]
async fn open_without_a_selected_record_cannot_start_a_new_session() {
    let mut request = picker_request();
    request.records.clear();
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut component = element!(SessionsPickerComponent(request,width:120usize,height:36usize,selected_outcome_out:&mut outcome));
    component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            [resume_key(), ctrl_key('d')],
        )))
        .collect::<Vec<_>>()
        .await;
    drop(component);
    assert!(
        outcome.is_none(),
        "R13: New has its own dialog; Open with no selected row must be inert: {outcome:?}"
    );
}
