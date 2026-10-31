#[cfg(test)]
mod tests {
    use crate::support::{
        dialog_scene::DialogScene,
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
        },
        controls::{Button, HelpControl, button_target},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    fn text(driver: &EventDriver) -> String {
        let grid = driver.frames.last().unwrap();
        if grid.cells.iter().flatten().any(|cell| cell.text == "╭") {
            return grid.round_frame_text();
        }
        (0..grid.height)
            .map(|row| grid.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[derive(Clone, Default)]
    struct HelpHandle(Arc<Mutex<Option<HelpFixture>>>);
    #[derive(Clone)]
    struct HelpFixture {
        control: HelpControl,
        enabled: State<bool>,
    }
    #[derive(Props)]
    struct HelpSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: HelpHandle,
    }
    const TITLES: [&str; 26] = [
        "Alpha action",
        "Bravo action",
        "Charlie action",
        "Delta action",
        "Echo action",
        "Foxtrot action",
        "Golf action",
        "Hotel action",
        "India action",
        "Juliet action",
        "Kilo action",
        "Lima action",
        "Mike action",
        "November action",
        "Oscar action",
        "Papa action",
        "Quebec action",
        "Romeo action",
        "Sierra action",
        "Tango action",
        "Uniform action",
        "Victor action",
        "Whiskey action",
        "Xray action",
        "Yankee action",
        "Zulu action",
    ];
    #[component]
    fn HelpScene(props: &HelpSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let enabled = hooks.use_state(|| true);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, action("help"));
            *props.handle.0.lock().unwrap() = Some(HelpFixture {
                control: help.clone(),
                enabled,
            });
            let open = ActionDef {
                id: action("help"),
                title: "Help",
                keys: vec!["F1".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: help.open_handler(Handler::from(|error| panic!("{error}"))),
            };
            let execute = ActionDef {
                id: action("execute"),
                title: "Execute",
                keys: vec!["F8".parse().unwrap()],
                availability: AvailabilityFn::new(move || {
                    if enabled.get() {
                        Availability::Available
                    } else {
                        Availability::Unavailable {
                            reason: "Already executed".into(),
                        }
                    }
                }),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    let mut enabled = enabled;
                    enabled.set(false);
                }),
            };
            let targets = vec![
                button_target(TargetId::for_action(&open.id), &open),
                button_target(TargetId::for_action(&execute.id), &execute),
            ];
            let mut actions = vec![open, execute];
            for (letter, title) in ('a'..='z').zip(TITLES) {
                actions.push(ActionDef {
                    id: action(title),
                    title,
                    keys: vec![format!("Ctrl+Alt+{letter}").parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Menu { section: "Actions" },
                    run: Handler::default(),
                });
            }
            SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("help-scene").unwrap(), actions).unwrap(),
                primary_action: None,
            }
        });
        let help = props
            .handle
            .0
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .control
            .clone();
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:30u16,quit_action:action("quit"),on_routed:move |_|{let snapshot=stack.snapshot();let mut surfaces=vec![(SurfaceId::Screen,vec![u8::from(enabled.get())])];for overlay in snapshot.overlays(){surfaces.push((SurfaceId::Overlay(overlay.id),overlay.model.focus.current().unwrap().as_str().as_bytes().to_vec()));}journal.processed(SurfaceSnapshot(surfaces));}) {
            OverlayHost {View {Button(id:TargetId::for_action(help.action()),action:help.action().clone()) Button(id:TargetId::for_action(&action("execute")),action:action("execute"))}}
        })
    }
    #[tokio::test]
    async fn main_help_projects_bound_pairs_updates_availability_and_scrolls_by_keys_and_wheel() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = HelpHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(HelpScene(journal,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let opened = driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                    .await;
                assert_eq!(opened.last().unwrap().snapshot.0.len(), 2);
                let top = text(&driver);
                assert!(
                    top.lines()
                        .any(|line| line.contains("F1") && line.contains("Help"))
                );
                assert!(
                    top.lines()
                        .any(|line| line.contains("F8") && line.contains("Execute"))
                );
                assert!(
                    top.lines()
                        .any(|line| line.contains("Ctrl+Alt+A") && line.contains(TITLES[0])),
                    "{top}"
                );
                let HelpFixture {
                    control: help,
                    mut enabled,
                } = handle.0.lock().unwrap().as_ref().unwrap().clone();
                let surface = opened.last().unwrap().snapshot.0[1].0;
                let scroll = help.scroll(surface).unwrap();
                enabled.set(false);
                driver.initial(&mut frames).await;
                assert!(text(&driver).lines().any(|line| line.contains("F8")
                    && line.contains("Execute")
                    && line.contains("Already executed")));
                let mut seen = TITLES
                    .into_iter()
                    .filter(|title| text(&driver).contains(title))
                    .collect::<Vec<_>>();
                for _ in 0..scroll
                    .position()
                    .maximum()
                    .div_ceil(scroll.position().viewport_height.max(1))
                {
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::PageDown), true)],
                            true,
                        )
                        .await;
                    let page = text(&driver);
                    for title in TITLES {
                        if page.contains(title) && !seen.contains(&title) {
                            seen.push(title);
                        }
                    }
                }
                assert_eq!(
                    seen.len(),
                    26,
                    "all bound actions must be reachable: {seen:?}"
                );
                assert!(text(&driver).contains(TITLES[25]));
                assert_eq!(scroll.position().offset, scroll.position().maximum());
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::PageUp), true),
                            (key(KeyCode::Up), true),
                            (key(KeyCode::Down), true),
                        ],
                        true,
                    )
                    .await;
                assert!(scroll.position().offset > 0);
                let before = scroll.position().offset;
                let wheel = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::ScrollUp,
                    20,
                    12,
                ));
                driver
                    .drive(&mut frames, mode, vec![(wheel, true)], true)
                    .await;
                assert_eq!(scroll.position().offset, before - 1);
                let closed = driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                    .await;
                assert_eq!(closed.last().unwrap().snapshot.0.len(), 1);
                assert!(!text(&driver).contains("Close Help"));
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn dialog_help_swaps_one_body_and_restores_query_focus_and_draft_byte_for_byte() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let data = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui =
                element!(DialogScene(journal,form:true,width:76u16,height:30u16,data:data.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let prepared = driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::Enter), true),
                            (TerminalEvent::BracketedPaste("remote".into()), true),
                            (key(KeyCode::Esc), true),
                            (key(KeyCode::Down), true),
                            (key(KeyCode::Left), true),
                            (TerminalEvent::BracketedPaste("X".into()), true),
                        ],
                        true,
                        |ack| {
                            if ack.sequence == 2 {
                                assert_eq!(
                                    ack.snapshot.0.len(),
                                    3,
                                    "the real dropdown must receive the query"
                                );
                                assert_eq!(ack.snapshot.0[2].1, b"remote|beta");
                            }
                        },
                    )
                    .await;
                let before = prepared.last().unwrap().snapshot.clone();
                let canvas_before = driver.frames.last().unwrap().clone();
                let data = data.lock().unwrap().unwrap();
                let query_before = data.client.read().clone();
                let draft_before = data.directory.read().clone();
                let opened = driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(1)), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0.len(),
                                2,
                                "Help must not add a second dialog"
                            );
                            assert_eq!(ack.snapshot.0[1].0, before.0[1].0);
                        },
                    )
                    .await;
                assert_eq!(opened.len(), 1);
                let help = text(&driver);
                assert!(help.contains("Close Help"));
                assert!(
                    help.lines()
                        .any(|line| line.contains("Ctrl+Enter") && line.contains("Start"))
                );
                assert!(
                    help.lines()
                        .any(|line| line.contains("Esc") && line.contains("Close"))
                );
                assert!(!help.contains("Connection"));
                let closed = driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                    .await;
                assert_eq!(closed.last().unwrap().snapshot, before);
                assert_eq!(*data.client.read(), query_before);
                assert_eq!(*data.directory.read(), draft_before);
                assert_eq!(driver.frames.last().unwrap(), &canvas_before);
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
