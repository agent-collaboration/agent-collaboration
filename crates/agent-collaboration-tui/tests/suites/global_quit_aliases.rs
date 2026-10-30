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
        controls::HelpControl,
        focus_model::FocusModel,
        input_owner::{InputOwner, Routed},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    };
    use iocraft::prelude::*;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    #[derive(Clone, Copy, Default)]
    enum AliasDeclaration {
        #[default]
        Absent,
        Global,
        Unregistered,
        LocalDomain,
    }
    #[derive(Clone, Copy, Default, Debug)]
    enum ScenePhase {
        #[default]
        Screen,
        Help,
        Dialog,
        Dropdown,
        Menu,
    }
    #[derive(Props)]
    struct AliasSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        declaration: AliasDeclaration,
        phase: ScenePhase,
        deny_quit: bool,
    }
    #[component]
    fn AliasScene(props: &AliasSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let quits = hooks.use_state(|| 0usize);
        let local_calls = hooks.use_state(|| 0usize);
        let help = hooks.use_ref(|| None::<HelpControl>);
        let deny = props.deny_quit;
        let stack = use_surface_stack(&mut hooks, |stack| {
            let mut keys = vec!["Ctrl+C".parse().unwrap()];
            if matches!(props.declaration, AliasDeclaration::Global) {
                keys.push("Ctrl+D".parse().unwrap());
            }
            let quit = ActionDef {
                toolbar_label: None,
                id: action("quit"),
                title: "Quit".into(),
                keys,
                availability: AvailabilityFn::new(move || {
                    if !deny && quits.get() == 0 {
                        Availability::Available
                    } else {
                        Availability::Unavailable {
                            reason: "Committing or already quit".into(),
                        }
                    }
                }),
                choice: None,
                placement: Placement::Menu {
                    section: "Application",
                },
                run: Handler::from(move |()| {
                    let mut quits = quits;
                    quits.set(quits.get() + 1)
                }),
            };
            let mut actions = vec![quit];
            if matches!(props.declaration, AliasDeclaration::LocalDomain) {
                actions.push(ActionDef {
                    toolbar_label: None,
                    id: action("domain-d"),
                    title: "Domain D".into(),
                    keys: vec!["Ctrl+D".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut calls = local_calls;
                        calls.set(calls.get() + 1)
                    }),
                });
            }
            let help_control = HelpControl::new(stack, action("help"));
            let mut retained = help;
            retained.set(Some(help_control));
            SurfaceModel {
                focus: FocusModel::new(Vec::new()).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("alias-screen").unwrap(), actions)
                    .unwrap(),
                primary_action: None,
            }
        });
        let mut opened = hooks.use_ref(|| false);
        if !opened.get() {
            let open = |kind| {
                stack
                    .open_overlay(OverlaySpec {
                        kind,
                        model: SurfaceModel {
                            focus: FocusModel::new(Vec::new()).unwrap(),
                            actions: ActionRegistry::new(
                                ScopeId::new("alias-overlay").unwrap(),
                                Vec::new(),
                            )
                            .unwrap(),
                            primary_action: None,
                        },
                        view: OverlayView::new(|| {
                            element!(View(width:100pct,height:100pct) {Text(content:"Overlay")})
                                .into_any()
                        }),
                        layout: OverlayLayout::AtCells {
                            left: 10,
                            top: 4,
                            width: 30,
                            height: 10,
                        },
                    })
                    .unwrap()
            };
            match props.phase {
                ScenePhase::Screen => {}
                ScenePhase::Help => {
                    help.read()
                        .as_ref()
                        .unwrap()
                        .show(Default::default())
                        .unwrap();
                }
                ScenePhase::Dialog => {
                    open(OverlayKind::Dialog);
                }
                ScenePhase::Dropdown => {
                    open(OverlayKind::Dialog);
                    open(OverlayKind::Dropdown {
                        field: agent_collaboration_tui::focus_model::TargetId::new("choice")
                            .unwrap(),
                    });
                }
                ScenePhase::Menu => {
                    open(OverlayKind::Menu);
                }
            };
            opened.set(true);
        }
        let aliases = if matches!(
            props.declaration,
            AliasDeclaration::Global | AliasDeclaration::Unregistered
        ) {
            vec!["Ctrl+D".parse().unwrap()]
        } else {
            Vec::new()
        };
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:70u16,height:24u16,quit_action:action("quit"),global_quit_aliases:aliases,on_routed:move |routed|{
            if routed==Routed::Quit {journal.log_handler("global quit");}
            journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",quits.get(),local_calls.get()).into_bytes())]));
        }) {OverlayHost {View(width:100pct,height:100pct) {Text(content:format!("Quits {} local {}",quits.get(),local_calls.get()))}}})
    }
    fn control_d(kind: KeyEventKind) -> TerminalEvent {
        let mut key = KeyEvent::new(kind, KeyCode::Char('d'));
        key.modifiers = KeyModifiers::CONTROL;
        TerminalEvent::Key(key)
    }
    #[tokio::test]
    async fn raw_control_c_reaches_the_global_quit_from_a_dialog_and_keeps_its_guard() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for deny_quit in [false, true] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(AliasScene(journal,phase:ScenePhase::Dialog,deny_quit));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::Char('\u{3}'),
                                )),
                                !deny_quit,
                            )],
                            !deny_quit,
                            |ack| {
                                assert_eq!(
                                    ack.snapshot.0[0].1,
                                    if deny_quit {
                                        b"0|0".as_slice()
                                    } else {
                                        b"1|0".as_slice()
                                    },
                                    "raw Ctrl+C reaches the screen guard from a dialog"
                                );
                                assert_eq!(ack.handlers.len(), usize::from(!deny_quit));
                            },
                        )
                        .await;
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn declared_quit_alias_reaches_screen_from_help_dialog_dropdown_and_menu_once() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for phase in [
                ScenePhase::Help,
                ScenePhase::Dialog,
                ScenePhase::Dropdown,
                ScenePhase::Menu,
            ] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui =
                    element!(AliasScene(journal,declaration:AliasDeclaration::Global,phase));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    driver.drive_checked(&mut frames,mode,vec![(control_d(KeyEventKind::Press),true),(control_d(KeyEventKind::Press),false)],true,|ack|{
                        assert_eq!(ack.snapshot.0[0].1,b"1|0","{phase:?}: declared alias must reach the screen quit guard before any redraw");
                        assert_eq!(ack.handlers,vec!["global quit"],"second press is denied by the same live guard");
                    }).await;
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn undeclared_or_unregistered_aliases_leave_overlay_routing_unchanged() {
        for declaration in [AliasDeclaration::Absent, AliasDeclaration::Unregistered] {
            for phase in [
                ScenePhase::Help,
                ScenePhase::Dialog,
                ScenePhase::Dropdown,
                ScenePhase::Menu,
            ] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(AliasScene(journal, declaration, phase));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    driver
                        .drive_checked(
                            &mut frames,
                            InputMode::Burst,
                            vec![(control_d(KeyEventKind::Press), false)],
                            false,
                            |ack| {
                                assert_eq!(ack.snapshot.0[0].1, b"0|0");
                                assert!(ack.handlers.is_empty());
                            },
                        )
                        .await;
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn declared_alias_keeps_the_committing_guard_and_ignores_repeat_and_release() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(AliasScene(journal,declaration:AliasDeclaration::Global,phase:ScenePhase::Help,deny_quit:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![
                        (control_d(KeyEventKind::Repeat), false),
                        (control_d(KeyEventKind::Release), false),
                        (control_d(KeyEventKind::Press), false),
                    ],
                    false,
                    |ack| {
                        assert_eq!(ack.snapshot.0[0].1, b"0|0");
                        assert!(ack.handlers.is_empty());
                    },
                )
                .await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn scope_without_alias_retains_its_own_ctrl_d_domain_action() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(AliasScene(journal,declaration:AliasDeclaration::LocalDomain,phase:ScenePhase::Screen));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(control_d(KeyEventKind::Press), true)],
                    true,
                    |ack| {
                        assert_eq!(ack.snapshot.0[0].1, b"0|1");
                        assert!(ack.handlers.is_empty());
                    },
                )
                .await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
