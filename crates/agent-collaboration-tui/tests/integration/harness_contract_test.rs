//! Cross-library integration: compare the recording adapter with the pinned renderer and exercise real event acknowledgements.
#[cfg(test)]
mod tests {

    use crate::test_support;
    use crate::test_support::recording_backend::recording_terminal_render_loop;
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        input::input_owner::InputOwner,
        surfaces::{
            focus_model::FocusModel,
            surface_handle::use_surface_stack,
            surface_stack::{SurfaceId, SurfaceModel},
        },
    };
    use iocraft::prelude::*;
    use test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
    };

    #[derive(Props)]
    struct HarnessProbeProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn HarnessProbe(props: &HarnessProbeProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let count = hooks.use_state(|| 0usize);
        let quit = hooks.use_state(|| false);
        let mut system = hooks.use_context_mut::<SystemContext>();
        let stack = use_surface_stack(&mut hooks, |_| {
            let action = |name: &str, key: &str, run: Handler<()>| ActionDef {
                toolbar_label: None,
                id: ActionId::new(name).unwrap(),
                title: "Probe".into(),
                keys: vec![key.parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run,
            };
            let journal = props.journal.clone();
            SurfaceModel {
                focus: FocusModel::new(Vec::new()).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("probe").unwrap(),
                    vec![
                        action(
                            "increment",
                            "a",
                            Handler::from(move |()| {
                                let mut count = count;
                                count.set(count.get() + 1);
                                journal.log_handler("increment");
                            }),
                        ),
                        action(
                            "quit",
                            "Ctrl+C",
                            Handler::from(move |()| {
                                let mut quit = quit;
                                quit.set(true);
                            }),
                        ),
                    ],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        if quit.get() {
            system.exit();
        }
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:20u16,height:2u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,count.get().to_le_bytes().to_vec())]))) {
            View(width:20,height:2,background_color:Color::Rgb {r:4,g:5,b:6}) {
                Text(content: format!("Count {}", count.get()),color:Color::Cyan,weight:Weight::Bold)
            }
        })
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }

    async fn parity_frames(recording: bool, mode: InputMode) -> Vec<FrameGrid> {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HarnessProbe(journal));
        let mut frames: std::pin::Pin<Box<dyn futures_util::Stream<Item = iocraft::Canvas> + '_>> =
            if recording {
                Box::pin(recording_terminal_render_loop(&mut ui, config))
            } else {
                // The sole A use of the old mock is its reference-parity observation.
                Box::pin(ui.mock_terminal_render_loop(config))
            };
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                mode,
                vec![
                    (key(KeyCode::Char('a')), true),
                    (key(KeyCode::F(12)), false),
                    (key(KeyCode::Char('a')), true),
                ],
                true,
            )
            .await;
        driver.frames
    }
    #[tokio::test]
    async fn recording_backend_has_frame_parity_with_the_pinned_mock_without_ctrl_c() {
        let scenario = async {
            for mode in [InputMode::Burst, InputMode::FrameSeparated] {
                assert_eq!(
                    parity_frames(true, mode).await,
                    parity_frames(false, mode).await,
                    "{mode:?}"
                );
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn burst_driver_delivers_all_events_before_any_redraw() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HarnessProbe(journal: journal.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let initial_frames = journal.draw_count();
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Char('a')), true); 3],
                    true,
                )
                .await;
            assert_eq!(acknowledgements.len(), 3);
            assert!(
                acknowledgements
                    .iter()
                    .all(|ack| ack.drawn_frames == initial_frames),
                "{acknowledgements:?}"
            );
            assert_eq!(
                acknowledgements.last().unwrap().snapshot,
                SurfaceSnapshot(vec![(SurfaceId::Screen, 3usize.to_le_bytes().to_vec())])
            );
            assert_eq!(
                acknowledgements.last().unwrap().handlers,
                ["increment", "increment", "increment"]
            );
            assert_eq!(driver.frames.last().unwrap().row_text(0).trim(), "Count 3");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn separated_driver_uses_processed_ack_for_an_ignored_event_without_waiting_for_redraw() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HarnessProbe(journal: journal.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![
                        (key(KeyCode::Char('a')), true),
                        (key(KeyCode::Char('a')), true),
                    ],
                    true,
                )
                .await;
            assert!(acknowledgements[1].drawn_frames > acknowledgements[0].drawn_frames);
            let frame_count = driver.frames.len();
            let draws = journal.draw_count();
            let ignored = driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(12)), false)],
                    false,
                )
                .await;
            assert_eq!(ignored[0].sequence, 3);
            assert_eq!(ignored[0].drawn_frames, draws);
            assert_eq!(driver.frames.len(), frame_count);
            assert_eq!(ignored[0].handlers, ["increment", "increment"]);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
