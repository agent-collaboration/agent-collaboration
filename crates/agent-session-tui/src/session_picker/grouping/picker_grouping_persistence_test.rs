//! Real SQLite loads and the mounted preference owner survive teardown and reopen.
use crate::session_picker::SessionGroupFacet;
use crate::session_picker::test_support::*;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;

async fn open_preference_run(
    directory: &Path,
    initial: SessionGroupFacet,
    cycle_once: bool,
    width: usize,
    height: usize,
) {
    let (preferences, writer) =
        crate::session_picker::screen::picker_entrypoint::load_group_preferences(Some(directory))
            .await;
    let writer = writer.map(|writer| tokio::spawn(writer.run()));
    let observed = Arc::new(Mutex::new(None));
    let surface_observer = observed.clone();
    let processed = Arc::new(AtomicUsize::new(0));
    let input_observer = processed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut request = picker_request();
    request.records.truncate(1);
    {
        let mut picker = element!(SessionsPickerComponent(
            request,width,height,group_preferences:Some(preferences),
            surface_observer:move |state|{*surface_observer.lock().unwrap()=Some(state)},
            input_observer:move |_|{input_observer.fetch_add(1,Ordering::Release);}
        ));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        for phase in 0..=usize::from(cycle_once) {
            let expected = if phase == 0 {
                initial
            } else {
                SessionGroupFacet::StartedBy
            };
            let frame = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(canvas) = frames.next().await {
                    if processed.load(Ordering::Acquire) < phase {
                        continue;
                    }
                    let frame = canvas.to_string();
                    let (stack, _): (SurfaceStackHandle, State<SessionsPickerModel>) =
                        observed.lock().unwrap().unwrap();
                    let snapshot = stack.snapshot();
                    let registry = &snapshot.surface(SurfaceId::Screen).unwrap().actions;
                    let checked = registry
                        .definitions()
                        .iter()
                        .find(|action| {
                            action.id.as_str() == format!("group-{}", expected.stored_value())
                        })
                        .unwrap()
                        .choice
                        .as_ref()
                        .unwrap()
                        .get();
                    let painted = if expected == SessionGroupFacet::None {
                        !frame.contains("(1)")
                    } else {
                        frame
                            .lines()
                            .any(|line| line.contains("You") && line.contains("(1)"))
                    };
                    if checked && painted {
                        return frame;
                    }
                }
                panic!("picker ended before remembered {expected:?}");
            })
            .await
            .unwrap_or_else(|_| panic!("waiting for remembered {expected:?} at {width}x{height}"));
            assert!(frame.contains("Feature design"), "{frame}");
            sender
                .send(if cycle_once && phase == 0 {
                    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(6)))
                } else {
                    ctrl_key('d')
                })
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while frames.next().await.is_some() {}
        })
        .await
        .expect("preference run exits after Quit");
    }
    if let Some(writer) = writer {
        tokio::time::timeout(Duration::from_secs(5), writer)
            .await
            .expect("preference writer drains after component teardown")
            .unwrap();
    }
}

#[tokio::test]
async fn grouping_reopens_saved_facet_and_opens_damaged_value_as_none_at_all_four_grids() {
    for (width, height) in [(120, 36), (76, 30), (48, 12), (32, 24)] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("service");
        open_preference_run(&directory, SessionGroupFacet::None, true, width, height).await;
        let database = directory.join("agent-sessions-ui.sqlite");
        let raw = ui_preference_store::UiPreferenceStore::open_and_migrate(&database).await;
        assert_eq!(
            raw.load("sessions", "group-by").await.as_deref(),
            Some("started-by")
        );
        raw.close().await;
        open_preference_run(
            &directory,
            SessionGroupFacet::StartedBy,
            false,
            width,
            height,
        )
        .await;
        let raw = ui_preference_store::UiPreferenceStore::open_and_migrate(&database).await;
        raw.save("sessions", "group-by", "removed-facet")
            .await
            .unwrap();
        raw.close().await;
        open_preference_run(&directory, SessionGroupFacet::None, false, width, height).await;
        let raw = ui_preference_store::UiPreferenceStore::open_and_migrate(&database).await;
        assert_eq!(
            raw.load("sessions", "group-by").await.as_deref(),
            Some("removed-facet")
        );
        raw.close().await;
        assert!(!directory.join("agent-proxy-ui.sqlite").exists());
    }
}

#[tokio::test]
async fn grouping_without_a_preference_directory_stays_transient() {
    let (mut preferences, writer) =
        crate::session_picker::screen::picker_entrypoint::load_group_preferences(None).await;
    assert!(writer.is_none());
    assert_eq!(preferences.facet(), SessionGroupFacet::None);
    preferences.choose(SessionGroupFacet::Folder);
    assert_eq!(preferences.facet(), SessionGroupFacet::Folder);
    let (reopened, writer) =
        crate::session_picker::screen::picker_entrypoint::load_group_preferences(None).await;
    assert!(writer.is_none());
    assert_eq!(reopened.facet(), SessionGroupFacet::None);
}
