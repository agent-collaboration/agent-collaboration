// R7: shared Help replaces the screen-specific Help title.
//! Actual renderer/controller/configured dispatcher status proof; default catalog is a stand-in.
use super::*;
use agent_session_inventory::test_support::source_api_fixture::SourceApiFixture;
use agent_session_inventory::test_support::source_fault_fixture::{ReadPeer, ReplyScenario};
use agent_session_inventory::{
    source_inventory_request::InventoryPageProgress, source_inventory_request::PickerSourceContext,
    source_inventory_request::SourceInventoryResult,
};

#[path = "default_inventory_publication_tests.rs"]
mod default_inventory_publication_tests;

#[path = "ssh_source_publication_tests.rs"]
mod ssh_source_publication_tests;

async fn capture_all_status(
    source: PickerSourceContext,
    actual: SessionsPickerRecordLoader,
    marker: &str,
    held: Option<&ReadPeer>,
) -> String {
    let PickerSourceContext::ConfiguredHosted(mut profile) = source else {
        panic!("configured fixture")
    };
    profile.name = "Observed source".to_owned().try_into().unwrap();
    let mut request = picker_request();
    request.root = SessionsPickerRoot::Any;
    request.source = SessionsSource::All;
    request.records.clear();
    request.router_registry = RouterRegistryRead::Ready(
        agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry {
            routers: vec![profile],
        },
    );
    let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
        let actual = actual.clone();
        Box::pin(async move {
            if request.source_context == PickerSourceContext::DefaultHosted {
                SourceInventoryResult::Ready {
                    request,
                    binding: None,
                    progress: InventoryPageProgress::Complete,
                    snapshot: observed_records(Vec::new()),
                }
            } else {
                actual(request).await
            }
        })
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(key(KeyCode::F(2))).unwrap();
    let input = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let held_ready = Arc::new(std::sync::atomic::AtomicBool::new(held.is_none()));
    let observed_held = held_ready.clone();
    let observer_sender = sender.clone();
    let observer = async {
        if let Some(peer) = held {
            peer.wait_for_held_page().await;
            observed_held.store(true, std::sync::atomic::Ordering::SeqCst);
            observer_sender.send(key(KeyCode::F(1))).unwrap();
        }
    };
    let mut last_frame = String::new();
    let render = tokio::time::timeout(Duration::from_secs(2), async {
        let mut picker = element! {SessionsPickerComponent(request,record_loader:Some(loader),width:180usize,height:60usize,selected_outcome_out:&mut outcome)};
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(input));
        tokio::pin!(frames);
        let mut selected = false;
        let mut captured = None;
        while let Some(canvas) = frames.next().await {
            let text = canvas.to_string();
            last_frame = text.clone();
            if !selected && text.contains("Choose machine") {
                sender.send(key(KeyCode::Enter)).unwrap();
                selected = true;
                sender.send(key(KeyCode::Down)).unwrap();
                sender.send(key(KeyCode::Enter)).unwrap();
            }
            if captured.is_none()
                && text.contains("sessions · All machines")
                && text.contains(marker)
                && held_ready.load(std::sync::atomic::Ordering::SeqCst)
            {
                captured = Some(text);
                sender.send(control_key('c')).unwrap();
            }
        }
        captured.expect("actual source status must be rendered")
    });
    let (frame, ()) = tokio::join!(render, observer);
    let frame = frame.unwrap_or_else(|error| {
        panic!("source status did not reach {marker}: {error}; last frame: {last_frame}")
    });
    assert!(outcome.is_none());
    frame
}

#[tokio::test]
async fn review_all_complete_real_source_is_not_reported_unavailable() {
    let source =
        SourceApiFixture::start("00000000-0000-4000-8000-000000000001", "source-model").await;
    let frame = capture_all_status(
        source.request("Observed source").source_context,
        source.loader(),
        "Observed source: 1 sessions",
        None,
    )
    .await;
    source.finish().await;
    assert!(
        !frame.contains("Partial view"),
        "completed sources are healthy: {frame}"
    );
    assert!(!frame.contains("connections need verification"));
    assert!(frame.contains("Source session"));
}

#[tokio::test]
async fn review_all_qualified_empty_source_is_not_unqualified() {
    let source = ReadPeer::start(ReplyScenario::RowBudget(0)).await;
    let frame = capture_all_status(
        source.request().source_context,
        source.loader(),
        "Observed source: 0 sessions",
        None,
    )
    .await;
    source.finish().await;
    assert!(
        !frame.contains("Partial view"),
        "a qualified empty result is not a rejected read: {frame}"
    );
}

#[tokio::test]
async fn review_all_held_source_reports_loading_not_false_unavailability() {
    let source = ReadPeer::start(ReplyScenario::HoldPage).await;
    let frame = capture_all_status(
        source.request().source_context,
        source.loader(),
        "Observed source: Loading",
        Some(&source),
    )
    .await;
    source.finish().await;
    assert!(
        frame.contains("Loading view"),
        "pending qualified read must remain loading: {frame}"
    );
    assert!(!frame.contains("unavailable: Observed source"));
}

#[tokio::test]
async fn review_all_rejection_uses_actual_failed_source_reason() {
    let source = ReadPeer::start(ReplyScenario::WrongPageEndpoint).await;
    let frame = capture_all_status(
        source.request().source_context,
        source.loader(),
        "Observed source: Unavailable",
        None,
    )
    .await;
    source.finish().await;
    assert!(frame.contains("Partial view") && frame.contains("unavailable: Observed source"));
    assert!(
        !frame.contains("connections need verification"),
        "failure must reflect the actual rejected read: {frame}"
    );
}

#[tokio::test]
async fn review_all_nonterminal_page_reports_unfinished_not_unavailable() {
    let source = ReadPeer::start(ReplyScenario::HoldSecondPage).await;
    let frame = capture_all_status(
        source.request().source_context,
        source.loader(),
        "1 fetched; unfinished",
        Some(&source),
    )
    .await;
    source.finish().await;
    assert!(
        frame.contains("Partial view") && frame.contains("unfinished: Observed source"),
        "unfinished pages remain partial: {frame}"
    );
    assert!(!frame.contains("unavailable: Observed source"));
}

#[tokio::test]
async fn review_machine_help_is_visible_in_collapsed_and_expanded_responsive_frames() {
    for width in [28usize, 40, 51, 52, 104, 108, 160] {
        for expanded in [false, true] {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let input = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let mut last_frame = String::new();
            let mut scroll_frame = String::new();
            let result = tokio::time::timeout(Duration::from_secs(2), async {
                let mut picker = element! {
                    SessionsPickerComponent(request: picker_request(), width, height: 60usize)
                };
                let frames =
                    picker.mock_terminal_render_loop(MockTerminalConfig::with_events(input));
                tokio::pin!(frames);
                let mut opened = false;
                let mut captured = None;
                while let Some(canvas) = frames.next().await {
                    let text = canvas.to_string();
                    last_frame = text.clone();
                    if expanded && !opened {
                        opened = true;
                        sender.send(key(KeyCode::F(1))).unwrap();
                        continue;
                    }
                    if expanded && text.contains("Close Help") && !text.contains("Machine…") {
                        if scroll_frame != text {
                            scroll_frame = text;
                            sender.send(key(KeyCode::Down)).unwrap();
                        }
                        continue;
                    }
                    if captured.is_none() && (!expanded || text.contains("Close Help")) {
                        captured = Some(text);
                        sender.send(control_key('c')).unwrap();
                    }
                }
                captured.expect("help frame")
            })
            .await;
            let frame = result
                .unwrap_or_else(|_| panic!("width={width} expanded={expanded}: {last_frame}"));
            assert!(
                if expanded {
                    frame.contains("Machine…")
                } else {
                    frame.contains("Menu F10")
                },
                "width{width}/expanded{expanded}: {frame}"
            );
        }
    }
}

#[tokio::test]
async fn configured_native_unavailability_renders_partial_notice_without_hiding_stored_rows() {
    let source = SourceApiFixture::start_with_provider(
        "00000000-0000-4000-8000-000000000001",
        "stored-model",
        true,
    )
    .await;
    let frame = capture_all_status(
        source.request("Observed source").source_context,
        source.loader(),
        "Native Claude inventory incomplete",
        None,
    )
    .await;
    assert!(
        frame.contains("Partial view"),
        "native failure must not be labelled complete: {frame}"
    );
    assert!(
        frame.contains("Source session"),
        "native Codex Stored row retained: {frame}"
    );
    assert!(
        frame.contains("Source provider"),
        "managed Stored row retained: {frame}"
    );
    source.finish().await;
}

#[tokio::test]
async fn configured_native_http_classification_reaches_the_actual_partial_or_rejected_view() {
    use agent_session_inventory::test_support::source_fault_fixture::native_partitions::NativeScenario;
    for scenario in [
        NativeScenario::Unavailable,
        NativeScenario::PrefixUnavailable,
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let frame = capture_all_status(
            peer.request().source_context,
            peer.loader(),
            "Native Claude inventory incomplete",
            None,
        )
        .await;
        assert!(frame.contains("Partial view"));
        assert!(
            frame.contains("Managed source"),
            "qualified Stored row retained: {frame}"
        );
        if matches!(scenario, NativeScenario::PrefixUnavailable) {
            assert!(
                frame.contains("Native equal ID"),
                "validated native prefix retained: {frame}"
            );
        }
        assert!(!frame.contains("NATIVE_PARTITION_PRIVATE_SENTINEL"));
        peer.finish().await;
    }
    for scenario in [
        NativeScenario::InvalidParams,
        NativeScenario::UnknownFailure,
        NativeScenario::MalformedFailure,
        NativeScenario::WrongErrorId,
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let frame = capture_all_status(
            peer.request().source_context,
            peer.loader(),
            "Observed source: Unavailable",
            None,
        )
        .await;
        assert!(
            !frame.contains("Managed source"),
            "invalid source rows withdrawn: {frame}"
        );
        assert!(
            !frame.contains("Native Claude inventory incomplete"),
            "invalid is not recoverable availability: {frame}"
        );
        assert!(!frame.contains("NATIVE_PARTITION_PRIVATE_SENTINEL"));
        peer.finish().await;
    }
}

#[tokio::test]
async fn actual_native_partial_and_invalid_publications_keep_captured_details_and_copy_target() {
    use agent_session_inventory::test_support::source_fault_fixture::native_partitions::NativeScenario;
    for (scenario, marker, single) in [
        (
            NativeScenario::PrefixHeldUnavailable,
            "Native Claude inventory incomplete",
            false,
        ),
        (
            NativeScenario::PrefixHeld,
            "Observed source: Unavailable",
            false,
        ),
        (NativeScenario::PrefixHeld, "sessions · This machine", true),
    ] {
        let peer = ReadPeer::start(ReplyScenario::NativePartition(scenario)).await;
        let PickerSourceContext::ConfiguredHosted(mut profile) = peer.request().source_context
        else {
            panic!("configured source")
        };
        profile.name = "Observed source".to_owned().try_into().unwrap();
        let mut request = picker_request();
        request.root = SessionsPickerRoot::Any;
        request.source = SessionsSource::All;
        request.records.clear();
        request.router_registry = RouterRegistryRead::Ready(
            agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry {
                routers: vec![profile],
            },
        );
        let actual = peer.loader();
        let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
            let actual = actual.clone();
            Box::pin(async move {
                if request.source_context == PickerSourceContext::DefaultHosted {
                    SourceInventoryResult::Ready {
                        request,
                        binding: None,
                        progress: InventoryPageProgress::Complete,
                        snapshot: observed_records(Vec::new()),
                    }
                } else {
                    actual(request).await
                }
            })
        });
        let copied = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let copier: crate::session_picker::session_id_clipboard::SessionClipboardWriter =
            Arc::new({
                let copied = copied.clone();
                move |session_id| {
                    copied.lock().unwrap().push(session_id.to_owned());
                    Ok(())
                }
            });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(key(KeyCode::F(2))).unwrap();
        let input = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let held_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observer = async {
            peer.wait_for_held_page().await;
            held_ready.store(true, std::sync::atomic::Ordering::SeqCst);
            sender.send(key(KeyCode::F(1))).unwrap();
        };
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let mut phase = 0;
        let mut last = String::new();
        let render = async {
            let mut picker = element!(SessionsPickerComponent(request,record_loader:Some(loader),session_clipboard_writer:Some(copier),width:180usize,height:60usize,selected_outcome_out:&mut outcome));
            let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(input));
            tokio::pin!(frames);
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Choose machine") {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender.send(key(KeyCode::Down)).unwrap();
                    if single {
                        sender.send(key(KeyCode::Down)).unwrap();
                    }
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 1;
                } else if phase == 1
                    && held_ready.load(std::sync::atomic::Ordering::SeqCst)
                    && frame.contains("Close Help")
                {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 2;
                } else if phase == 2
                    && !frame.contains("Close Help")
                    && frame.contains("Native equal ID")
                {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Native equal ID"))
                        .unwrap();
                    let byte = line.find("Native equal ID").unwrap();
                    let column = unicode_width::UnicodeWidthStr::width(line.get(..byte).unwrap());
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Moved,
                            u16::try_from(column)
                                .expect("captured fixture coordinates fit terminal cells"),
                            u16::try_from(row)
                                .expect("captured fixture coordinates fit terminal cells"),
                        )))
                        .unwrap();
                    phase = 3;
                } else if phase == 3 && frame.contains("❯ Native equal ID") {
                    sender.send(key(KeyCode::F(4))).unwrap();
                    phase = 4;
                } else if phase == 4
                    && frame.contains("Session details")
                    && frame.contains("/source/native")
                {
                    assert!(
                        frame.contains("Machine: Observed source"),
                        "captured source must be explicit in Details: {frame}"
                    );
                    assert!(
                        frame.contains("State: Idle"),
                        "native Details must use the observed native state: {frame}"
                    );
                    assert!(copied.lock().unwrap().is_empty());
                    peer.release_held_page().await;
                    phase = 5;
                } else if phase == 5 && frame.contains(marker) {
                    assert!(
                        frame.contains("Session details")
                            && frame.contains("Native equal ID")
                            && frame.contains("/source/native"),
                        "background publication cannot replace the captured native origin: {frame}"
                    );
                    assert!(!frame.contains("NATIVE_PARTITION_PRIVATE_SENTINEL"));
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 6;
                } else if phase == 6 && frame.contains("Copy requested") {
                    sender.send(control_key('d')).unwrap();
                    phase = 7;
                }
                last = frame;
            }
        };
        let (result, ()) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(3), render),
            observer
        );
        assert!(result.is_ok(), "phase={phase}: {last}");
        assert_eq!(phase, 7);
        assert_eq!(
            copied.lock().unwrap().as_slice(),
            ["00000000-0000-4000-8000-000000000042"]
        );
        assert!(outcome.is_none());
        peer.finish().await;
    }
}
