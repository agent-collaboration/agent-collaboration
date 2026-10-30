#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Checked, Placement,
            ScopeId,
        },
        controls::{
            ActionProjections, Button, HelpControl, MenuButton, Toolbar, button_target,
            menu_overlay, toolbar_actions, toolbar_target,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn id(value: &str) -> ActionId {
        ActionId::new(value).unwrap()
    }
    fn definition(
        name: &str,
        title: &'static str,
        key: &str,
        placement: Placement,
        run: Handler<()>,
    ) -> ActionDef {
        ActionDef {
            toolbar_label: None,
            id: id(name),
            title: title.into(),
            keys: vec![key.parse().unwrap()],
            placement,
            run,
            availability: AvailabilityFn::default(),
            choice: None,
        }
    }
    #[derive(Props)]
    struct ActionsSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn ActionsScene(props: &ActionsSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let count = hooks.use_state(|| 0usize);
        let checked = hooks.use_state(|| false);
        let retained_help = hooks.use_ref(|| Arc::new(Mutex::new(None))).read().clone();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, id("help"));
            *retained_help.lock().unwrap() = Some(help.clone());
            let help = definition(
                "help",
                "Help",
                "F1",
                Placement::Menu { section: "Help" },
                help.open_handler(
                    agent_collaboration_tui::controls::HelpContent::default,
                    Handler::from(|error| panic!("{error}")),
                ),
            );
            let mut start = definition(
                "start",
                "Start",
                "F8",
                Placement::Toolbar,
                Handler::from(move |()| {
                    let mut count = count;
                    count.set(count.get() + 1);
                }),
            );
            start.availability = AvailabilityFn::new(move || {
                if count.get() == 0 {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Already running".into(),
                    }
                }
            });
            let mut setting = definition(
                "setting",
                "All sessions",
                "F6",
                Placement::Menu { section: "View" },
                Handler::from(move |()| {
                    let mut checked = checked;
                    checked.set(!checked.get());
                }),
            );
            setting.choice = Some(Checked::new(move || checked.get()));
            let second = definition(
                "second",
                "Second action",
                "F9",
                Placement::Toolbar,
                Handler::default(),
            );
            let menu = definition(
                "menu",
                "Menu",
                "F10",
                Placement::Hidden,
                Handler::from(move |()| {
                    let state = stack.snapshot();
                    let registry = &state.surface(SurfaceId::Screen).unwrap().actions;
                    stack
                        .open_overlay(
                            menu_overlay(
                                stack,
                                registry,
                                OverlayLayout::AtCells {
                                    left: 15,
                                    top: 3,
                                    width: 32,
                                    height: 8,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }),
            );
            let registry = ActionRegistry::new(
                ScopeId::new("actions").unwrap(),
                vec![menu.clone(), start.clone(), second.clone(), setting, help],
            )
            .unwrap();
            let targets = vec![
                button_target(TargetId::for_action(&id("menu")), &menu),
                toolbar_target(&start, registry.clone(), move || {
                    stack.viewport().0.saturating_sub(13)
                }),
                toolbar_target(&second, registry.clone(), move || {
                    stack.viewport().0.saturating_sub(13)
                }),
                button_target(TargetId::new("inline-start").unwrap(), &start),
            ];
            SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions: registry,
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:70u16,height:15u16,quit_action:id("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",count.get(),checked.get()).into_bytes())]))) {
            OverlayHost {View(width:100pct,flex_direction:FlexDirection::Column) {
                Toolbar(available_width:stack.viewport().0.saturating_sub(13)) {MenuButton(id:TargetId::for_action(&id("menu")),action:id("menu"))}
                Button(id:TargetId::new("inline-start").unwrap(),action:id("start"))
            }}
        })
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    fn text(driver: &EventDriver) -> String {
        (0..driver.frames.last().unwrap().cells.len())
            .map(|row| driver.frames.last().unwrap().row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn menu_text(driver: &EventDriver) -> String {
        let grid = driver.frames.last().unwrap();
        (3..11)
            .map(|row| {
                grid.cells[row][15..47]
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[tokio::test]
    async fn effects_recheck_live_availability_inside_a_burst_and_every_projection_updates() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ActionsScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                assert!(text(&driver).contains("Start F8"));
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(8)), true), (key(KeyCode::F(8)), false)],
                        true,
                        |ack| assert_eq!(ack.snapshot.0[0].1, b"1|false"),
                    )
                    .await;
                assert_eq!(text(&driver).matches("Already running").count(), 2);
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                let menu = menu_text(&driver);
                assert!(
                    menu.contains("Start")
                        && menu.contains("F8")
                        && menu.contains("Already running"),
                    "{menu}"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn menu_is_reachable_by_tab_and_enter_and_shows_model_checkmarks() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ActionsScene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::F(6)), false), (key(KeyCode::Enter), true)],
                    true,
                )
                .await;
            let menu = menu_text(&driver);
            assert!(
                menu.contains("All sessions") && menu.contains('✓') && menu.contains("F6"),
                "{menu}"
            );
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![
                        (key(KeyCode::Esc), true),
                        (key(KeyCode::Tab), true),
                        (key(KeyCode::Tab), true),
                        (key(KeyCode::Tab), true),
                        (key(KeyCode::Tab), true),
                        (key(KeyCode::Enter), true),
                    ],
                    true,
                )
                .await;
            assert!(text(&driver).contains("All sessions"));
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
    #[test]
    fn action_projections_preserve_names_keys_and_every_action_when_toolbar_shrinks() {
        let registry = ActionRegistry::new(
            ScopeId::new("projection").unwrap(),
            vec![
                definition(
                    "one",
                    "First action",
                    "F4",
                    Placement::Toolbar,
                    Handler::default(),
                ),
                definition(
                    "two",
                    "Second action",
                    "F5",
                    Placement::Toolbar,
                    Handler::default(),
                ),
            ],
        )
        .unwrap();
        let projections = ActionProjections::from_registry(&registry);
        for projection in [
            &projections.menu,
            &projections.toolbar,
            &projections.help,
            &projections.hints,
        ] {
            assert_eq!(
                projection
                    .iter()
                    .map(|action| (action.title.as_str(), action.keys[0].to_string()))
                    .collect::<Vec<_>>(),
                [
                    ("First action", "F4".into()),
                    ("Second action", "F5".into())
                ]
            );
        }
        assert_eq!(toolbar_actions(&registry, 100).len(), 2);
        assert_eq!(toolbar_actions(&registry, 18).len(), 1);
        assert!(toolbar_actions(&registry, 2).is_empty());
        assert_eq!(ActionProjections::from_registry(&registry).menu.len(), 2);
    }
    #[tokio::test]
    async fn help_chosen_from_the_menu_opens_for_the_original_screen_scope() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ActionsScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::F(10)), true),
                            (key(KeyCode::Down), true),
                            (key(KeyCode::Down), true),
                            (key(KeyCode::Down), true),
                            (key(KeyCode::Enter), true),
                        ],
                        true,
                    )
                    .await;
                let help = driver.frames.last().unwrap().round_frame_text();
                assert!(help.contains("Close Help"), "{help}");
                assert!(
                    help.lines()
                        .any(|line| line.contains("F8") && line.contains("Start")),
                    "{help}"
                );
                assert!(
                    help.lines()
                        .any(|line| line.contains("F10") && line.contains("Menu")),
                    "{help}"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
