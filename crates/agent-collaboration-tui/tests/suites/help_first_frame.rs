#[cfg(all(test, feature = "test-support", debug_assertions))]
mod tests {
    use crate::support::{
        dispatch_frame_scene::{SharedFrameOutput, dispatch_render_loop},
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot},
        frame_grid::FrameGrid,
        list_scene::{key, target},
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{HelpContent, HelpControl},
        focus_model::{FocusModel, TargetSpec},
        input_dispatch_receipt::InputDispatchReceiptSink,
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::SurfaceModel,
        theme::UiGlyph,
    };
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    #[derive(Props)]
    struct FirstHelpSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        extra_actions: usize,
        fixture: Arc<Mutex<Option<(HelpControl, SurfaceStackHandle)>>>,
        sink: Option<InputDispatchReceiptSink>,
    }
    #[component]
    fn FirstHelpScene(
        props: &FirstHelpSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, ActionId::new("help").unwrap());
            *props.fixture.lock().unwrap() = Some((help.clone(), stack));
            let mut actions = vec![ActionDef {
                id: help.action().clone(),
                title: "Help".into(),
                toolbar_label: None,
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: help.open_handler(
                    HelpContent::default,
                    Handler::from(|error| panic!("{error}")),
                ),
            }];
            actions.extend((1..=props.extra_actions).map(|number| ActionDef {
                id: ActionId::new(format!("action-{number}")).unwrap(),
                title: "Declared action".into(),
                toolbar_label: None,
                keys: Vec::new(),
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::default(),
            }));
            SurfaceModel {
                focus: FocusModel::new(vec![TargetSpec::new(target("body"))]).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("first-help").unwrap(), actions).unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),dispatch_receipt:props.sink.clone(),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {
            OverlayHost {Text(content:"First Help fixture")}
        })
    }

    async fn first_help(width: u16, height: u16, extra_actions: usize, expected_overflow: bool) {
        let directory = tempfile::tempdir().unwrap();
        let sink = InputDispatchReceiptSink::create(directory.path().join("input-dispatch.jsonl"))
            .unwrap();
        let (mut driver, config, journal) = EventDriver::new();
        let fixture = Arc::new(Mutex::new(None));
        let mut ui = element!(FirstHelpScene(journal,width,height,extra_actions,fixture:fixture.clone(),sink:Some(sink.clone())));
        let output = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(output.clone()).unwrap());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured = observed.clone();
        let mut frames = Box::pin(
            dispatch_render_loop(&mut ui, config, output, writer)
                .inspect(move |frame| {
                    captured.lock().unwrap().push((
                        frame
                            .canvas
                            .as_ref()
                            .map(|canvas| FrameGrid::from_canvas(canvas).unwrap()),
                        frame.bytes.clone(),
                    ));
                })
                .filter_map(|frame| async move { frame.canvas }),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let before = observed.lock().unwrap().len();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(1)), true)],
                    true,
                )
                .await;
            let (help, stack) = fixture.lock().unwrap().clone().unwrap();
            let surface = stack.snapshot().top_id();
            let scroll = help.scroll(surface).unwrap();
            assert_eq!(
                scroll.position().maximum() > 0,
                expected_overflow,
                "fixture must reach declared overflow case"
            );
            let (first, bytes) = observed.lock().unwrap().get(before).unwrap().clone();
            let first = first.as_ref().unwrap();
            let text = (0..first.height)
                .map(|row| first.row_text(row))
                .collect::<Vec<_>>()
                .join("\n");
            eprintln!("FIRST F1 {width}x{height}, overflow={expected_overflow}:\n{text}");
            assert!(bytes.ends_with(b"\x1b[?2026l\x1b]7770;dispatch=1\x1b\\"));
            assert_eq!(
                text.contains(UiGlyph::ScrollThumb.text()),
                expected_overflow,
                "first marked Help frame must expose overflow\n{text}"
            );
            let content = help.content(surface).unwrap();
            assert_eq!(
                content
                    .conventions
                    .iter()
                    .map(|row| (row.key.as_str(), row.description.as_str()))
                    .collect::<Vec<_>>(),
                vec![
                    ("PgUp/PgDn · Option+↑/↓", "Page"),
                    ("Home/End · Option+Shift+↑/↓", "First/last")
                ]
            );
            if expected_overflow {
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::End), true)],
                        true,
                    )
                    .await;
            }
            let final_grid = driver.frames.last().unwrap();
            let all_rows = (0..final_grid.height)
                .map(|row| final_grid.row_text(row))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(all_rows.contains("Option+↑/↓"), "{all_rows}");
            assert!(all_rows.contains("Option+Shift+↑/↓"), "{all_rows}");
            assert!(
                all_rows.contains("Page") && all_rows.contains("First/last"),
                "{all_rows}"
            );
            sink.flush_and_check().unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn f1_first_frame_exposes_overflow_at_120x36() {
        first_help(120, 36, 40, true).await;
    }
    #[tokio::test]
    async fn f1_first_frame_exposes_overflow_at_48x12() {
        first_help(48, 12, 40, true).await;
    }
    #[tokio::test]
    async fn f1_first_frame_keeps_track_absent_when_content_fits() {
        first_help(120, 36, 0, false).await;
    }
}
