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
        controls::{
            Button, HelpContent, HelpContextLine, HelpControl, HelpInputConvention, button_target,
            menu_overlay,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
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
        stack: SurfaceStackHandle,
    }
    #[derive(Props)]
    struct HelpSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: HelpHandle,
        content: HelpContent,
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
        let content = props.content.clone();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, action("help"));
            *props.handle.0.lock().unwrap() = Some(HelpFixture {
                control: help.clone(),
                enabled,
                stack,
            });
            let open = ActionDef {
                toolbar_label: None,
                id: action("help"),
                title: "Help".into(),
                keys: vec!["F1".parse().unwrap(), "Ctrl+7".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: help.open_handler(
                    move || content.clone(),
                    Handler::from(|error| panic!("{error}")),
                ),
            };
            let execute = ActionDef {
                toolbar_label: None,
                id: action("execute"),
                title: "Execute".into(),
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
                    toolbar_label: None,
                    id: action(title),
                    title: title.into(),
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
                    ..
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
    async fn raw_unit_separator_opens_help_with_printable_bindings_in_help_and_menu() {
        let handle = HelpHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HelpScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let raw =
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('\u{1f}')));
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(raw, true)],
                    true,
                    |ack| {
                        assert_eq!(
                            ack.snapshot.0.len(),
                            2,
                            "raw US must open Help before redraw"
                        )
                    },
                )
                .await;
            assert!(text(&driver).contains("Ctrl+7"));
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .cells
                    .iter()
                    .flatten()
                    .all(|cell| !cell.text.chars().any(char::is_control))
            );
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Esc), true)],
                    true,
                )
                .await;
            let fixture = handle.0.lock().unwrap().as_ref().unwrap().clone();
            fixture
                .stack
                .open_overlay(
                    menu_overlay(
                        fixture.stack,
                        &fixture.stack.snapshot().top().actions,
                        OverlayLayout::AtCells {
                            left: 3,
                            top: 3,
                            width: 68,
                            height: 20,
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
            driver.initial(&mut frames).await;
            let menu = text(&driver);
            assert!(
                menu.lines()
                    .any(|line| line.contains("Help") && line.contains("Ctrl+7")),
                "{menu}"
            );
            assert!(
                driver
                    .frames
                    .last()
                    .unwrap()
                    .cells
                    .iter()
                    .flatten()
                    .all(|cell| !cell.text.chars().any(char::is_control))
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
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
    #[tokio::test]
    async fn context_and_input_conventions_scroll_with_the_generated_action_list() {
        let content = HelpContent {
            context: vec![
                HelpContextLine {
                    label: "Account".into(),
                    value: "account-42".into(),
                },
                HelpContextLine {
                    label: "Label".into(),
                    value: "Full captured label for this account".into(),
                },
            ],
            conventions: vec![
                HelpInputConvention {
                    key: "Arrows".into(),
                    description: "Move selection".into(),
                },
                HelpInputConvention {
                    key: "PgUp / PgDn".into(),
                    description: "Move a page".into(),
                },
                HelpInputConvention {
                    key: "Home / End".into(),
                    description: "First or last choice".into(),
                },
                HelpInputConvention {
                    key: "Typing".into(),
                    description: "Filter the list".into(),
                },
                HelpInputConvention {
                    key: "Backspace".into(),
                    description: "Edit the query".into(),
                },
                HelpInputConvention {
                    key: "id: / b: / repo:".into(),
                    description: "Search syntax".into(),
                },
                HelpInputConvention {
                    key: "Esc ladder".into(),
                    description: "Clear, close, return".into(),
                },
            ],
        };
        let handle = HelpHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HelpScene(journal,handle:handle.clone(),content));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let opened = driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::F(1)), true)],
                    true,
                )
                .await;
            let first = text(&driver);
            assert!(first.contains("account-42"), "{first}");
            assert!(
                first.contains("Full captured label for this account"),
                "{first}"
            );
            let fixture = handle.0.lock().unwrap().clone().unwrap();
            let scroll = fixture
                .control
                .scroll(opened.last().unwrap().snapshot.0[1].0)
                .unwrap();
            assert!(scroll.position().maximum() > 0);
            let mut pages = vec![first];
            for _ in 0..scroll
                .position()
                .maximum()
                .div_ceil(scroll.position().viewport_height.max(1))
            {
                driver
                    .drive(
                        &mut frames,
                        InputMode::Burst,
                        vec![(key(KeyCode::PageDown), true)],
                        true,
                    )
                    .await;
                pages.push(text(&driver));
            }
            let all = pages.join("\n");
            assert!(all.contains("Input conventions"), "{all}");
            assert!(all.contains("Esc ladder"), "{all}");
            assert!(all.contains("Clear, close, return"), "{all}");
            assert!(all.contains("Search syntax"), "{all}");
            for title in TITLES {
                assert!(all.contains(title), "generated action missing: {title}");
            }
            assert!(
                all.lines()
                    .any(|line| line.contains("F8") && line.contains("Execute"))
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
