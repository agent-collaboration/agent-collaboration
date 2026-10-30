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
        focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
        theme::{SurfaceRole, TextRole},
    };
    use iocraft::prelude::*;

    #[derive(Clone, Copy)]
    struct ExceptionState {
        current_value: State<String>,
        start_count: State<usize>,
        quit_count: State<usize>,
        quit_allowed: State<bool>,
    }
    fn target_id(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn action_id(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn action(name: &str, key: &str, run: impl Fn(()) + Send + Sync + 'static) -> ActionDef {
        ActionDef {
            toolbar_label: None,
            id: action_id(name),
            title: "Action".into(),
            keys: vec![key.parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(run),
        }
    }
    fn choices(
        stack: SurfaceStackHandle,
        state: ExceptionState,
        journal: EventJournal,
        kind: OverlayKind,
    ) -> OverlaySpec {
        let group = GroupId::new("options").unwrap();
        let targets = ["first", "second"]
            .into_iter()
            .map(|name| {
                TargetSpec::new(target_id(name))
                    .in_group(group.clone())
                    .with_activation(action_id("pick"))
            })
            .collect();
        let pick = action("pick", "Enter", move |()| {
            let mut value = state.current_value;
            value.set("picked".into());
            journal.log_handler("pick");
            stack.close_top();
        });
        OverlaySpec {
            kind,
            model: SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("choices").unwrap(), vec![pick]).unwrap(),
                primary_action: None,
            },
            view: OverlayView::new(|| element!(ExceptionBody).into_any()),
            layout: OverlayLayout::AtCells {
                left: 12,
                top: 5,
                width: 24,
                height: 6,
            },
        }
    }
    fn dialog(
        stack: SurfaceStackHandle,
        state: ExceptionState,
        journal: EventJournal,
    ) -> OverlaySpec {
        let mut start = action("start", "Ctrl+Enter", {
            let journal = journal.clone();
            move |()| {
                let mut count = state.start_count;
                count.set(count.get() + 1);
                journal.log_handler("start");
            }
        });
        start.availability = AvailabilityFn::new(move || {
            if state.start_count.get() == 0 {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "already started".into(),
                }
            }
        });
        let dropdown = action("dropdown", "F3", {
            let journal = journal.clone();
            move |()| {
                stack
                    .open_overlay(choices(
                        stack,
                        state,
                        journal.clone(),
                        OverlayKind::Dropdown {
                            field: target_id("field"),
                        },
                    ))
                    .unwrap();
            }
        });
        let menu = action("menu", "F10", move |()| {
            stack
                .open_overlay(choices(stack, state, journal.clone(), OverlayKind::Menu))
                .unwrap();
        });
        let group = GroupId::new("fields").unwrap();
        OverlaySpec {
            kind: OverlayKind::Dialog,
            model: SurfaceModel {
                focus: FocusModel::new(vec![
                    TargetSpec::new(target_id("field"))
                        .in_group(group.clone())
                        .with_activation(action_id("dropdown")),
                    TargetSpec::new(target_id("start-button"))
                        .in_group(group)
                        .with_activation(action_id("start")),
                ])
                .unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("dialog").unwrap(),
                    vec![start, dropdown, menu],
                )
                .unwrap(),
                primary_action: Some(action_id("start")),
            },
            view: OverlayView::new(|| element!(ExceptionBody).into_any()),
            layout: OverlayLayout::AtCells {
                left: 6,
                top: 2,
                width: 34,
                height: 12,
            },
        }
    }
    #[derive(Props)]
    struct ExceptionSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        deny_quit: bool,
    }
    #[component]
    fn ExceptionScene(
        props: &ExceptionSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let state = ExceptionState {
            current_value: hooks.use_state(|| "original".into()),
            start_count: hooks.use_state(|| 0),
            quit_count: hooks.use_state(|| 0),
            quit_allowed: hooks.use_state(|| !props.deny_quit),
        };
        let stack = use_surface_stack(&mut hooks, |stack| {
            let mut quit = action("quit", "Ctrl+C", move |()| {
                let mut count = state.quit_count;
                count.set(count.get() + 1);
                let mut allowed = state.quit_allowed;
                allowed.set(false);
            });
            quit.availability = AvailabilityFn::new(move || {
                if !state.quit_allowed.get() {
                    Availability::Unavailable {
                        reason: "committing".into(),
                    }
                } else {
                    Availability::Available
                }
            });
            let dialog_action = action("dialog", "F2", {
                let journal = props.journal.clone();
                move |()| {
                    stack
                        .open_overlay(dialog(stack, state, journal.clone()))
                        .unwrap();
                }
            });
            let dropdown = action("dropdown", "F3", {
                let journal = props.journal.clone();
                move |()| {
                    stack
                        .open_overlay(choices(
                            stack,
                            state,
                            journal.clone(),
                            OverlayKind::Dropdown {
                                field: target_id("field"),
                            },
                        ))
                        .unwrap();
                }
            });
            let menu = action("menu", "F10", {
                let journal = props.journal.clone();
                move |()| {
                    stack
                        .open_overlay(choices(stack, state, journal.clone(), OverlayKind::Menu))
                        .unwrap();
                }
            });
            let direct = action("direct", "Ctrl+Enter", move |()| {
                let mut count = state.start_count;
                count.set(count.get() + 1);
            });
            SurfaceModel {
                focus: FocusModel::new(vec![TargetSpec::new(target_id("field"))]).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("screen").unwrap(),
                    vec![quit, dialog_action, dropdown, menu, direct],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element! {
            InputOwner(stack,width:60u16,height:18u16,quit_action:action_id("quit"),on_routed:move |_| {
                let mut snapshots=vec![(SurfaceId::Screen,format!("{}|{}|{}",state.current_value.read().as_str(),state.start_count.get(),state.quit_count.get()).into_bytes())];
                snapshots.extend(stack.snapshot().overlays().iter().map(|overlay|(SurfaceId::Overlay(overlay.id),format!("{:?}",overlay.kind).into_bytes())));
                journal.processed(SurfaceSnapshot(snapshots));
            }) { OverlayHost {ExceptionScreen(state)} }
        }
    }
    #[derive(Props)]
    struct ExceptionScreenProps {
        #[iocraft(required)]
        state: ExceptionState,
    }
    #[component]
    fn ExceptionScreen(
        props: &ExceptionScreenProps,
        hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        element!(View(width:100pct,height:100pct){Text(content:format!("{} | {} | {}",props.state.current_value.read().as_str(),props.state.start_count.get(),props.state.quit_count.get()),color:context.theme().foreground(TextRole::Text))})
    }
    #[component]
    fn ExceptionBody(hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let context = *hooks.use_context::<SurfaceContext>();
        let state = context.stack.snapshot();
        let surface = state.surface(context.surface).unwrap();
        let theme = context.theme();
        element!(View(width:100pct,height:100pct,border_style:BorderStyle::Single,background_color:theme.background(SurfaceRole::Raised),flex_direction:FlexDirection::Column){
            Text(content:surface.actions.scope().as_str().to_owned(),color:theme.foreground(TextRole::Text))
            #(surface.focus.targets().iter().map(|target|element!(HitRegion(target:target.id.clone(),kind:HitRegionKind::Target){
                View(width:100pct,height:1){Text(content:format!("{} {}",if context.is_focused(&target.id){"❯"}else{" "},target.id.as_str()),color:theme.foreground(TextRole::Text))}
            })))
        })
    }
    #[derive(Clone, Copy, Debug)]
    enum Phase {
        Screen,
        Menu,
        Dialog,
        DialogDropdown,
        ScreenDropdown,
        DialogMenu,
    }
    impl Phase {
        fn opening(self) -> Vec<(TerminalEvent, bool)> {
            match self {
                Self::Screen => Vec::new(),
                Self::Menu => vec![(key(KeyCode::F(10)), true)],
                Self::Dialog => vec![(key(KeyCode::F(2)), true)],
                Self::DialogDropdown => {
                    vec![(key(KeyCode::F(2)), true), (key(KeyCode::F(3)), true)]
                }
                Self::ScreenDropdown => vec![(key(KeyCode::F(3)), true)],
                Self::DialogMenu => vec![(key(KeyCode::F(2)), true), (key(KeyCode::F(10)), true)],
            }
        }
        fn layers(self) -> usize {
            match self {
                Self::Screen => 1,
                Self::DialogDropdown | Self::DialogMenu => 3,
                _ => 2,
            }
        }
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    fn control(code: KeyCode) -> TerminalEvent {
        let mut event = KeyEvent::new(KeyEventKind::Press, code);
        event.modifiers = KeyModifiers::CONTROL;
        TerminalEvent::Key(event)
    }

    #[tokio::test]
    async fn ctrl_c_reaches_the_screen_action_from_every_surface_and_respects_its_live_availability()
     {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for deny_quit in [false, true] {
                for phase in [
                    Phase::Screen,
                    Phase::Menu,
                    Phase::Dialog,
                    Phase::DialogDropdown,
                    Phase::ScreenDropdown,
                    Phase::DialogMenu,
                ] {
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(ExceptionScene(journal, deny_quit));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    let scenario = async {
                        driver.initial(&mut frames).await;
                        let mut events = phase.opening();
                        let first_command = events.len() + 1;
                        events.push((control(KeyCode::Char('c')), !deny_quit));
                        events.push((control(KeyCode::Char('c')), false));
                        let observations = driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                events,
                                !matches!(phase, Phase::Screen) || !deny_quit,
                                |ack| {
                                    if ack.sequence >= first_command {
                                        assert_eq!(
                                            ack.snapshot.0[0].1,
                                            if deny_quit {
                                                b"original|0|0".as_slice()
                                            } else {
                                                b"original|0|1".as_slice()
                                            },
                                            "{phase:?} / {mode:?} / unavailable={deny_quit}"
                                        );
                                    }
                                },
                            )
                            .await;
                        let final_state = observations.last().unwrap();
                        assert_eq!(
                            final_state.snapshot.0[0].1,
                            if deny_quit {
                                b"original|0|0".as_slice()
                            } else {
                                b"original|0|1".as_slice()
                            },
                            "{phase:?} / {mode:?} / unavailable={deny_quit}"
                        );
                        assert_eq!(final_state.snapshot.0.len(), phase.layers());
                    };
                    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                        .await
                        .unwrap();
                }
            }
        }
    }
    #[tokio::test]
    async fn ctrl_enter_cancels_only_a_dialogs_own_dropdown_and_runs_its_primary_once() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for phase in [
                Phase::Dialog,
                Phase::DialogDropdown,
                Phase::ScreenDropdown,
                Phase::DialogMenu,
                Phase::Menu,
            ] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(ExceptionScene(journal));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    let mut events = phase.opening();
                    if matches!(
                        phase,
                        Phase::DialogDropdown
                            | Phase::ScreenDropdown
                            | Phase::DialogMenu
                            | Phase::Menu
                    ) {
                        events.push((key(KeyCode::Down), true));
                    }
                    let should_start = matches!(phase, Phase::Dialog | Phase::DialogDropdown);
                    let first_command = events.len() + 1;
                    events.extend([
                        (control(KeyCode::Enter), should_start),
                        (control(KeyCode::Enter), false),
                    ]);
                    let observations = driver
                        .drive_checked(&mut frames, mode, events, true, |ack| {
                            if ack.sequence >= first_command {
                                assert_eq!(
                                    ack.snapshot.0[0].1,
                                    if should_start {
                                        b"original|1|0".as_slice()
                                    } else {
                                        b"original|0|0".as_slice()
                                    },
                                    "{phase:?} / {mode:?}"
                                );
                            }
                        })
                        .await;
                    let final_state = observations.last().unwrap();
                    assert_eq!(
                        final_state.snapshot.0[0].1,
                        if should_start {
                            b"original|1|0".as_slice()
                        } else {
                            b"original|0|0".as_slice()
                        },
                        "{phase:?} / {mode:?}"
                    );
                    assert_eq!(
                        final_state.snapshot.0.len(),
                        if should_start { 2 } else { phase.layers() }
                    );
                    assert_eq!(
                        final_state
                            .handlers
                            .iter()
                            .filter(|name| name.as_str() == "start")
                            .count(),
                        usize::from(should_start)
                    );
                    assert!(!final_state.handlers.iter().any(|name| name == "pick"));
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }
}
