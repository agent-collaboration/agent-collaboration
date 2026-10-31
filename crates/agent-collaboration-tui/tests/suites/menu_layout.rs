#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
        },
        controls::{HelpControl, MenuButton, button_target, menu_overlay},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn definition(name: &str, title: &'static str, keys: &[&str], run: Handler<()>) -> ActionDef {
        ActionDef {
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: keys.iter().map(|key| key.parse().unwrap()).collect(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Menu {
                section: "Commands",
            },
            run,
        }
    }
    #[derive(Props)]
    struct MenuSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
    }
    #[component]
    fn MenuScene(props: &MenuSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let help = hooks.use_ref(|| None::<HelpControl>);
        let calls = hooks.use_state(|| 0usize);
        let menu_height = props.height.saturating_sub(2).min(14);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help_control = HelpControl::new(stack, action("help"));
            let mut retained = help;
            retained.set(Some(help_control.clone()));
            let help_action = definition(
                "help",
                "Help",
                &["F1", "Ctrl+7", "Ctrl+_", "Ctrl+/"],
                help_control
                    .open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            );
            let new = definition(
                "new",
                "New background session…",
                &["F8"],
                Handler::default(),
            );
            let refresh = definition(
                "refresh",
                "Refresh",
                &["F5"],
                Handler::from(move |()| {
                    let mut calls = calls;
                    calls.set(calls.get() + 1);
                }),
            );
            let mut unavailable = definition(
                "unavailable",
                "Unavailable action",
                &["Ctrl+Shift+F9"],
                Handler::from(|()| panic!("unavailable action ran")),
            );
            unavailable.availability = AvailabilityFn::new(|| Availability::Unavailable {
                reason: "Waiting for selected machine capabilities".into(),
            });
            let mut menu = definition(
                "menu",
                "Menu",
                &["F10"],
                Handler::from(move |()| {
                    let registry = stack
                        .snapshot()
                        .surface(SurfaceId::Screen)
                        .unwrap()
                        .actions
                        .clone();
                    stack
                        .open_overlay(
                            menu_overlay(
                                stack,
                                &registry,
                                OverlayLayout::AtCells {
                                    left: 1,
                                    top: 1,
                                    width: 18,
                                    height: menu_height,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }),
            );
            menu.placement = Placement::Hidden;
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(TargetId::for_action(&menu.id), &menu)])
                    .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("menu-layout").unwrap(),
                    vec![menu, new, refresh, help_action, unavailable],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_| journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),calls.get().to_le_bytes().to_vec())]))) {
            OverlayHost {MenuButton(id:TargetId::for_action(&action("menu")),action:action("menu"))}
        })
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[tokio::test]
    async fn menu_widens_before_truncating_auxiliary_columns_and_keeps_each_title_on_one_row() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [(120u16, 36u16), (48, 12), (32, 24)] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(MenuScene(journal, width, height));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                        .await;
                    let menu = driver.frames.last().unwrap().round_frame_text();
                    for title in [
                        "New background session…",
                        "Refresh",
                        "Help",
                        "Unavailable action",
                    ] {
                        assert_eq!(
                            menu.lines().filter(|line| line.contains(title)).count(),
                            1,
                            "{width}x{height} {mode:?}: title intact on one row: {title}\n{menu}"
                        );
                    }
                    let help_row = menu.lines().find(|line| line.contains("Help")).unwrap();
                    assert!(
                        help_row.contains("F1") && !help_row.contains("Ctrl+"),
                        "primary key only in Menu\n{menu}"
                    );
                    let unavailable = menu
                        .lines()
                        .find(|line| line.contains("Unavailable action"))
                        .unwrap();
                    if width == 120 {
                        assert!(
                            unavailable.contains("Waiting for selected machine capabilities")
                                && unavailable.contains("Ctrl+Shift+F9"),
                            "{menu}"
                        );
                    } else {
                        assert!(
                            unavailable.contains('…'),
                            "auxiliary columns must ellipsize\n{menu}"
                        );
                    }
                    let ack = driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::F(5)), true)], true)
                        .await;
                    assert_eq!(
                        ack[0].snapshot.0[0],
                        (SurfaceId::Screen, 1usize.to_le_bytes().to_vec()),
                        "menu action retains its live handler and closes menu"
                    );
                    if width == 120 {
                        driver
                            .drive(
                                &mut frames,
                                mode,
                                vec![(key(KeyCode::F(10)), true), (key(KeyCode::F(1)), true)],
                                true,
                            )
                            .await;
                        let help = driver.frames.last().unwrap().round_frame_text();
                        for alias in ["F1", "Ctrl+7", "Ctrl+_", "Ctrl+/"] {
                            assert!(help.contains(alias), "Help retains alias {alias}\n{help}");
                        }
                    }
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    async fn menu_focus_keeps_one_left_marker_and_no_right_edge_duplicate() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(MenuScene(journal,width:76u16,height:30u16));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                for title in ["New background session…", "Refresh"] {
                    let panel = driver.frames.last().unwrap().round_frame_text();
                    assert_eq!(
                        panel.matches('❯').count(),
                        1,
                        "only the left marker shows focus\n{panel}"
                    );
                    assert!(
                        panel
                            .lines()
                            .any(|line| line.starts_with("│❯") && line.contains(title)),
                        "{panel}"
                    );
                    if title != "Refresh" {
                        driver
                            .drive(&mut frames, mode, vec![(key(KeyCode::Down), true)], true)
                            .await;
                    }
                }
            })
            .await
            .unwrap();
        }
    }
}
