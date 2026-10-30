#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, ActionTitle, Availability, AvailabilityFn,
            Placement, ScopeId,
        },
        controls::{
            DialogKeyHints, HelpControl, MenuButton, Toolbar, button_target, menu_overlay,
            toolbar_actions,
        },
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
    fn definition(
        name: &str,
        title: &'static str,
        key: &str,
        placement: Placement,
        run: Handler<()>,
    ) -> ActionDef {
        ActionDef {
            id: action(name),
            title: title.into(),
            toolbar_label: None,
            keys: vec![key.parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement,
            run,
        }
    }
    #[derive(Props)]
    struct LabelSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn LabelScene(props: &LabelSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let calls = hooks.use_state(|| 0usize);
        let help = hooks.use_ref(|| None::<HelpControl>);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help_control = HelpControl::new(stack, action("help"));
            let mut retained = help;
            retained.set(Some(help_control.clone()));
            let help_action = definition(
                "help",
                "Help",
                "F1",
                Placement::Menu { section: "Help" },
                help_control
                    .open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            );
            let mut new = definition(
                "new",
                "New session…",
                "F8",
                Placement::Toolbar,
                Handler::from(move |()| {
                    let mut calls = calls;
                    calls.set(calls.get() + 1);
                }),
            );
            new.toolbar_label = Some(ActionTitle::dynamic(move || {
                if calls.get() == 0 {
                    "+ New".to_owned()
                } else {
                    "+ Again".to_owned()
                }
            }));
            new.availability = AvailabilityFn::new(move || {
                if calls.get() == 0 {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Already running".into(),
                    }
                }
            });
            let menu = definition(
                "menu",
                "Menu",
                "F10",
                Placement::Hidden,
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
                                    left: 4,
                                    top: 5,
                                    width: 60,
                                    height: 12,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }),
            );
            SurfaceModel {
                focus: FocusModel::new(vec![
                    button_target(TargetId::for_action(&menu.id), &menu),
                    button_target(TargetId::for_action(&new.id), &new),
                ])
                .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("toolbar-labels").unwrap(),
                    vec![menu, new, help_action],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:80u16,height:28u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,calls.get().to_le_bytes().to_vec())]))) {
            OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
                Text(content:format!("Calls {}",calls.get()))
                Toolbar(available_width:80u16) {MenuButton(id:TargetId::for_action(&action("menu")),action:action("menu"))}
                DialogKeyHints(actions:vec![action("new")])
            }}
        })
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[tokio::test]
    async fn toolbar_short_label_stays_separate_from_menu_help_and_hints_with_one_binding() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(LabelScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                assert!(
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .row_text(1)
                        .contains("+ New F8")
                );
                assert!(
                    !driver
                        .frames
                        .last()
                        .unwrap()
                        .row_text(1)
                        .contains("New session…")
                );
                assert!(
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .row_text(2)
                        .contains("F8 New session…")
                );
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                let menu = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    menu.contains("New session…") && menu.contains("F8") && !menu.contains("+ New"),
                    "{menu}"
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true), (key(KeyCode::F(1)), true)],
                        true,
                    )
                    .await;
                let help = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    help.contains("New session…") && help.contains("F8") && !help.contains("+ New"),
                    "{help}"
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true), (key(KeyCode::F(8)), true)],
                        true,
                    )
                    .await;
                let toolbar = driver.frames.last().unwrap().row_text(1);
                assert!(toolbar.contains("+ Again F8 Already running"), "{toolbar}");
                let acknowledgements = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::F(8)), false),
                            (key(KeyCode::F(10)), true),
                            (key(KeyCode::F(8)), false),
                        ],
                        true,
                    )
                    .await;
                assert!(
                    acknowledgements
                        .iter()
                        .all(|ack| ack.snapshot.0[0].1 == 1usize.to_le_bytes()),
                    "disabled action cannot run from toolbar key or menu"
                );
                let menu = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    menu.contains("New session…")
                        && menu.contains("F8")
                        && menu.contains("Already running")
                        && !menu.contains("+ Again"),
                    "{menu}"
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true), (key(KeyCode::F(1)), true)],
                        true,
                    )
                    .await;
                let help = driver.frames.last().unwrap().round_frame_text();
                assert!(
                    help.contains("New session…")
                        && help.contains("Already running")
                        && !help.contains("+ Again"),
                    "{help}"
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[test]
    fn narrow_toolbar_budget_uses_the_short_label_it_will_paint() {
        let mut new = definition(
            "new",
            "New session…",
            "F8",
            Placement::Toolbar,
            Handler::default(),
        );
        new.toolbar_label = Some("+ New".into());
        let registry =
            ActionRegistry::new(ScopeId::new("short-budget").unwrap(), vec![new]).unwrap();
        assert_eq!(
            toolbar_actions(&registry, 11).len(),
            1,
            "5 label cells + 3 key/gap cells + 3 button cells"
        );
        assert!(toolbar_actions(&registry, 10).is_empty());
    }
}
