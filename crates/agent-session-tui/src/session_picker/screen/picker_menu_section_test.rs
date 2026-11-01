//! The Lead's T2 order is independent of the shared projection implementation.
use crate::session_picker::test_support::*;
use agent_collaboration_tui::controls::ActionProjections;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::OverlayKind;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;

struct MenuCapture {
    toolbar: Vec<String>,
    items: Vec<String>,
    headings: Vec<(String, Option<Vec<String>>, bool)>,
}
fn capture_menu(stack: SurfaceStackHandle) -> MenuCapture {
    let snapshot = stack.snapshot();
    let registry = &snapshot.surface(SurfaceId::Screen).unwrap().actions;
    let projection = ActionProjections::from_registry(registry);
    let headings = ["cycle-scope", "cycle-runtime", "cycle-sort", "cycle-group"]
        .into_iter()
        .map(|id| {
            let keys = snapshot
                .top()
                .actions
                .definitions()
                .iter()
                .find(|action| action.id.as_str() == id)
                .map(|action| action.keys.iter().map(ToString::to_string).collect());
            let target = snapshot
                .top()
                .focus
                .targets()
                .iter()
                .any(|target| target.id.as_str() == format!("action:{id}"));
            (id.to_owned(), keys, target)
        })
        .collect();
    MenuCapture {
        toolbar: projection
            .toolbar
            .into_iter()
            .map(|item| item.id.as_str().to_owned())
            .collect(),
        items: projection
            .menu
            .into_iter()
            .map(|item| item.id.as_str().to_owned())
            .collect(),
        headings,
    }
}

#[tokio::test]
async fn picker_menu_sections_follow_toolbar_scope_view_actions_and_application_order() {
    let observed = Arc::new(Mutex::new(None));
    let observer = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(TerminalEvent::Key(KeyEvent::new(
            KeyEventKind::Press,
            KeyCode::F(10),
        )))
        .unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker = element!(SessionsPickerComponent(
        request:picker_request(),width:120usize,height:36usize,
        surface_observer:move |(stack,_): (SurfaceStackHandle,State<SessionsPickerModel>)|{*observer.lock().unwrap()=Some(stack)}
    ));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let capture = tokio::time::timeout(Duration::from_secs(5), async {
        while frames.next().await.is_some() {
            let stack = observed.lock().unwrap().unwrap();
            if stack
                .snapshot()
                .overlays()
                .last()
                .is_some_and(|overlay| overlay.kind == OverlayKind::Menu)
            {
                // Resolve every state-backed presentation while its owner is mounted.
                return capture_menu(stack);
            }
        }
        panic!("picker ended before menu publication");
    })
    .await
    .expect("menu publishes after F10");
    sender.send(ctrl_key('d')).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while frames.next().await.is_some() {}
    })
    .await
    .expect("picker exits after Quit");
    assert_eq!(capture.toolbar, ["new", "focus-filter"]);
    assert_eq!(
        capture.items,
        [
            "new",
            "focus-filter",
            "scope-folder",
            "scope-repo",
            "scope-all",
            "runtime-all",
            "runtime-active",
            "runtime-idle",
            "runtime-blocked",
            "sort-updated",
            "sort-created",
            "group-none",
            "group-started-by",
            "group-day",
            "group-folder",
            "group-provider",
            "group-machine",
            "new-background",
            "details",
            "open",
            "fork",
            "refresh",
            "filters",
            "machine",
            "help",
            "quit",
        ]
    );
    for ((heading, keys, target), expected_key) in capture
        .headings
        .into_iter()
        .zip(["Ctrl+S", "Ctrl+T", "Ctrl+O", "F6"])
    {
        assert_eq!(
            keys,
            Some(vec![expected_key.to_owned()]),
            "section keeps existing key: {heading}"
        );
        assert!(!target, "heading must not become a focus target: {heading}");
    }
}

#[tokio::test]
async fn picker_menu_sections_reveal_quit_at_all_four_grids() {
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 12), (32, 24)] {
        let observed = Arc::new(Mutex::new(None));
        let observer = observed.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut picker = element!(SessionsPickerComponent(
            request:picker_request(),width,height,
            surface_observer:move |(stack,_): (SurfaceStackHandle,State<SessionsPickerModel>)|{*observer.lock().unwrap()=Some(stack)}
        ));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        sender.send(menu_key(KeyCode::F(10))).unwrap();
        let item_count = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Some(canvas) = frames.next().await else {panic!("picker ended before menu geometry at {width}x{height}")};
                let stack = observed.lock().unwrap().unwrap();
                let snapshot = stack.snapshot();
                if !snapshot.overlays().last().is_some_and(|overlay|overlay.kind == OverlayKind::Menu) {
                    continue;
                }
                let Some(regions) = stack.hit_regions().published() else {continue};
                let Some(viewport) = regions.targets.iter().find(|region|region.surface == snapshot.top_id() && region.target.as_str() == "menu-scroll" && !region.cells.is_empty()) else {continue};
                let rows = viewport.cells.iter().map(|(_, row)|*row).collect::<std::collections::BTreeSet<_>>();
                let frame = canvas.to_string();
                let drawn_rows = frame.lines().collect::<Vec<_>>();
                assert!(rows.iter().all(|row|drawn_rows.get(usize::from(*row)).is_some()), "{width}x{height}: {frame}");
                let scrollbar = rows.iter().any(|row|drawn_rows.get(usize::from(*row)).is_some_and(|line|line.contains('█')));
                println!("MENU_VIEWPORT {width}x{height}: items=26 headings=4 separators=6 borders=2 rows_needed=38 interior_available={} scrollbar={scrollbar}\n{frame}",rows.len());
                assert!(scrollbar, "the declared38-row menu overflows at {width}x{height}: {frame}");
                break ActionProjections::from_registry(&snapshot.top().actions).menu.len();
            }
        }).await.expect("actual menu viewport publishes after F10");
        for _ in 1..item_count {
            sender.send(menu_key(KeyCode::Down)).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(canvas) = frames.next().await {
                let stack = observed.lock().unwrap().unwrap();
                if stack
                    .snapshot()
                    .top()
                    .focus
                    .current()
                    .is_some_and(|target| target.as_str() == "action:quit")
                    && canvas.to_string().contains("Quit")
                {
                    return;
                }
            }
            panic!("picker ended before the last menu item at {width}x{height}");
        })
        .await
        .expect("Down reaches the final menu item in its live viewport");
        sender.send(ctrl_key('d')).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while frames.next().await.is_some() {}
        })
        .await
        .expect("Quit exits after the final menu observation");
    }
}
fn menu_key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
