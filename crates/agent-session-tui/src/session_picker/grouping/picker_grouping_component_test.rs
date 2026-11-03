//! Grouping must reach the mounted list through the real F6 action.
use crate::session_picker::test_support::*;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;

async fn grouping_cycle_at(width: usize, height: usize) {
    let mut request = picker_request();
    request.records.truncate(1);
    let identity = request.records[0].identity.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let observed = Arc::new(Mutex::new(None));
    let surface_observer = observed.clone();
    let processed = Arc::new(AtomicUsize::new(0));
    let input_observer = processed.clone();
    let mut picker = element!(SessionsPickerComponent(
        request,width,height,
        surface_observer:move |state| {*surface_observer.lock().unwrap()=Some(state)},
        input_observer:move |_| {input_observer.fetch_add(1,Ordering::Release);}
    ));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    for (phase, (facet, heading)) in [
        ("none", None),
        ("started-by", Some("You")),
        ("day", Some("Older")),
        ("folder", Some("/repo/project-a")),
        ("provider", Some("Codex")),
        ("machine", Some("This machine")),
        ("none", None),
    ]
    .into_iter()
    .enumerate()
    {
        let frame = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(canvas) = frames.next().await {
                if processed.load(Ordering::Acquire) < phase {
                    continue;
                }
                let frame = canvas.to_string();
                if heading.map_or_else(
                    || !frame.contains("(1)"),
                    |heading| frame.contains(heading) && frame.contains("(1)"),
                ) {
                    return frame;
                }
            }
            panic!("picker ended before Group by {facet}");
        })
        .await
        .unwrap_or_else(|_| panic!("waiting for F6 Group by {facet} at {width}x{height}"));
        let (stack, model): (SurfaceStackHandle, State<SessionsPickerModel>) =
            observed.lock().unwrap().unwrap();
        let snapshot = stack.snapshot();
        let actions = &snapshot.surface(SurfaceId::Screen).unwrap().actions;
        let checked = actions
            .definitions()
            .iter()
            .filter(|action| action.id.as_str().starts_with("group-"))
            .filter(|action| action.choice.as_ref().is_some_and(|choice| choice.get()))
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(checked, [format!("group-{facet}")], "{frame}");
        assert_eq!(model.read().focused_identity(), Some(&identity));
        sender
            .send(if phase == 6 {
                ctrl_key('d')
            } else {
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(6)))
            })
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while frames.next().await.is_some() {}
    })
    .await
    .expect("picker exits after Quit");
}

#[tokio::test]
async fn grouping_f6_cycles_every_facet_at_120x36() {
    grouping_cycle_at(120, 36).await;
}
#[tokio::test]
async fn grouping_f6_cycles_every_facet_at_76x30() {
    grouping_cycle_at(76, 30).await;
}
#[tokio::test]
async fn grouping_f6_cycles_every_facet_at_48x12() {
    grouping_cycle_at(48, 12).await;
}
#[tokio::test]
async fn grouping_f6_cycles_every_facet_at_32x24() {
    grouping_cycle_at(32, 24).await;
}
