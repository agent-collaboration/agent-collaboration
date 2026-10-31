#[cfg(test)]
mod tests {
    use crate::support::{
        dialog_scene::DialogScene,
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{
            ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, HelpControl, OptionKey,
            SelectOption, button_target,
        },
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn key(code: KeyCode, modifiers: KeyModifiers) -> TerminalEvent {
        let mut event = KeyEvent::new(KeyEventKind::Press, code);
        event.modifiers = modifiers;
        TerminalEvent::Key(event)
    }
    fn toggle(index: usize) -> TerminalEvent {
        match index {
            0 => key(KeyCode::F(1), KeyModifiers::NONE),
            1 => key(KeyCode::Char('7'), KeyModifiers::CONTROL),
            _ => key(KeyCode::Char('\u{1f}'), KeyModifiers::NONE),
        }
    }
    fn assert_help(stack: SurfaceStackHandle, help: &HelpControl, parent: SurfaceId) {
        assert_eq!(
            stack.snapshot().top_id(),
            parent,
            "Help replaces the parent dialog body"
        );
        assert!(
            help.is_active(parent),
            "dropdown Help opens through its actual scope action"
        );
        let source = help.source(parent).unwrap();
        assert_eq!(
            source.scope().as_str(),
            "dropdown",
            "Help projects its dropdown source"
        );
        assert!(
            source
                .definitions()
                .iter()
                .all(|definition| definition.id.as_str() != "start"
                    && definition.id.as_str() != "close"),
            "parent form actions do not leak into dropdown Help"
        );
    }
    #[tokio::test]
    async fn queried_and_empty_dropdowns_open_help_and_toggle_back_with_their_original_identity_and_state()
     {
        for (width, height) in [
            (120u16, 36u16),
            (76, 30),
            (48, 24),
            (48, 12),
            (40, 24),
            (32, 24),
        ] {
            for mode in [InputMode::Burst, InputMode::FrameSeparated] {
                for no_match in [false, true] {
                    for alias in 0..4 {
                        let stack_out = Arc::new(Mutex::new(None));
                        let help_out = Arc::new(Mutex::new(None));
                        let data_out = Arc::new(Mutex::new(None));
                        let (mut driver, config, journal) = EventDriver::new();
                        let mut ui = element!(DialogScene(journal,form:true,width,height,surface_stack:stack_out.clone(),help_control:help_out.clone(),data:data_out.clone()));
                        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                        tokio::time::timeout(std::time::Duration::from_secs(5), async {
                            driver.initial(&mut frames).await;
                            let stack = stack_out.lock().unwrap().unwrap();
                            let help = help_out.lock().unwrap().clone().unwrap();
                            let data = data_out.lock().unwrap().unwrap();
                            let parent = stack.snapshot().top_id();
                            let query = if no_match {
                                "zz-route-absent"
                            } else {
                                "remote"
                            };
                            driver
                                .drive(
                                    &mut frames,
                                    mode,
                                    vec![
                                        (key(KeyCode::Enter, KeyModifiers::NONE), true),
                                        (TerminalEvent::BracketedPaste(query.into()), true),
                                    ],
                                    true,
                                )
                                .await;
                            let popup = stack.snapshot().top_id();
                            let choice = data.client.read().clone();
                            let draft = data.directory.read().clone();
                            let focus = stack.snapshot().top().focus.current().cloned();
                            let before = driver.frames.last().unwrap().clone();
                            assert_ne!(popup, parent);
                            assert_eq!(choice.query().text(), query);
                            assert_eq!(choice.highlight().is_none(), no_match);
                            let opening = toggle(alias.min(2));
                            driver
                                .drive_checked(
                                    &mut frames,
                                    mode,
                                    vec![(opening, true)],
                                    true,
                                    |_| assert_help(stack, &help, parent),
                                )
                                .await;
                            let help_scroll=help.scroll(parent).unwrap();
                        if help_scroll.position().maximum()>0 {
                            driver.drive_checked(&mut frames,mode,vec![(key(KeyCode::PageDown,KeyModifiers::NONE),true)],true,|_| {
                                assert_eq!(*data.client.read(),choice,"scrolling Help does not edit its suspended dropdown");
                                assert_eq!(*data.directory.read(),draft);
                            }).await;
                            assert!(help_scroll.position().offset>0,"overflowing dropdown Help actually pages");
                        }
                        assert_eq!(*data.client.read(), choice);
                            assert_eq!(*data.directory.read(), draft);
                            assert_eq!(data.starts.get(), 0);
                            let closing = if alias == 3 {
                                key(KeyCode::Esc, KeyModifiers::NONE)
                            } else {
                                toggle(alias)
                            };
                            driver
                                .drive_checked(
                                    &mut frames,
                                    mode,
                                    vec![(closing, true)],
                                    true,
                                    |_| {
                                        assert!(!help.is_active(parent));
                                        assert_eq!(
                                            stack.snapshot().top_id(),
                                            popup,
                                            "the same dropdown identity returns"
                                        );
                                        assert_eq!(
                                            stack.snapshot().top().focus.current(),
                                            focus.as_ref()
                                        );
                                        assert_eq!(*data.client.read(), choice);
                                        assert_eq!(*data.directory.read(), draft);
                                        assert_eq!(data.starts.get(), 0);
                                    },
                                )
                                .await;
                            assert_eq!(
                                driver.frames.last().unwrap(),
                                &before,
                                "closing Help restores the exact popup grid"
                            );
                            if no_match {
                                driver
                                    .drive_checked(
                                        &mut frames,
                                        mode,
                                        vec![(key(KeyCode::Enter, KeyModifiers::NONE), false)],
                                        false,
                                        |_| {
                                            assert_eq!(stack.snapshot().top_id(), popup);
                                            assert_eq!(data.starts.get(), 0);
                                            assert_eq!(*data.client.read(), choice);
                                        },
                                    )
                                    .await;
                            }
                        })
                        .await
                        .unwrap();
                    }
                }
            }
        }
    }
    #[derive(Clone)]
    struct ScreenFixture {
        stack: SurfaceStackHandle,
        help: HelpControl,
        value: State<ComboBoxState>,
    }
    #[derive(Props)]
    struct ScreenSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: Arc<Mutex<Option<ScreenFixture>>>,
    }
    #[component]
    fn ScreenScene(props: &ScreenSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| {
            ComboBoxState::new(
                vec![
                    SelectOption::available(OptionKey::new("alpha").unwrap(), "Alpha remote"),
                    SelectOption::available(OptionKey::new("beta").unwrap(), "Beta remote"),
                ],
                Some(OptionKey::new("alpha").unwrap()),
            )
            .unwrap()
        });
        let field = TargetId::new("screen-choice").unwrap();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let help = HelpControl::new(stack, ActionId::new("help").unwrap());
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                parent_scroll: None,
                field: field.clone(),
                value,
                on_pick: Handler::default(),
            });
            let open = ActionDef {
                id: ActionId::new("open").unwrap(),
                title: "Choose".into(),
                toolbar_label: None,
                keys: vec![],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::from(move |()| {
                    control.open().unwrap();
                }),
            };
            let help_action = ActionDef {
                id: ActionId::new("help").unwrap(),
                title: "Help".into(),
                toolbar_label: None,
                keys: vec!["F1".parse().unwrap(), "Ctrl+7".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: help.open_handler(Default::default, Handler::from(|error| panic!("{error}"))),
            };
            *props.handle.lock().unwrap() = Some(ScreenFixture { stack, help, value });
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(field.clone(), &open)]).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("screen-help-dropdown").unwrap(),
                    vec![open, help_action],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:30u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),value.read().query().text().as_bytes().to_vec())]))) {OverlayHost {View(width:35) {ComboBox(id:field,value)}}})
    }
    #[tokio::test]
    async fn screen_dropdown_help_uses_its_source_and_returns_the_same_popup() {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ScreenScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![
                        (key(KeyCode::Enter, KeyModifiers::NONE), true),
                        (TerminalEvent::BracketedPaste("remote".into()), true),
                    ],
                    true,
                )
                .await;
            let popup = fixture.stack.snapshot().top_id();
            let before = driver.frames.last().unwrap().clone();
            let choice = fixture.value.read().clone();
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(toggle(1), true)],
                    true,
                    |_| {
                        let surface = fixture.stack.snapshot().top_id();
                        assert_ne!(surface, popup);
                        assert!(fixture.help.is_active(surface));
                        assert_eq!(
                            fixture.help.source(surface).unwrap().scope().as_str(),
                            "dropdown"
                        );
                    },
                )
                .await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(toggle(2), true)],
                    true,
                    |_| {
                        assert_eq!(fixture.stack.snapshot().top_id(), popup);
                        assert_eq!(*fixture.value.read(), choice);
                    },
                )
                .await;
            assert_eq!(driver.frames.last().unwrap(), &before);
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn late_option_updates_reach_the_suspended_popup_and_new_pointer_targets_on_return() {
        let stack_out = Arc::new(Mutex::new(None));
        let help_out = Arc::new(Mutex::new(None));
        let data_out = Arc::new(Mutex::new(None));
        let control_out = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(DialogScene(journal,form:true,width:76u16,height:30u16,surface_stack:stack_out.clone(),help_control:help_out.clone(),data:data_out.clone(),combo:control_out.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let stack = stack_out.lock().unwrap().unwrap();
            let help = help_out.lock().unwrap().clone().unwrap();
            let data = data_out.lock().unwrap().unwrap();
            let parent = stack.snapshot().top_id();
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![
                        (key(KeyCode::Enter, KeyModifiers::NONE), true),
                        (TerminalEvent::BracketedPaste("remote".into()), true),
                    ],
                    true,
                )
                .await;
            let popup = stack.snapshot().top_id();
            let control = control_out.lock().unwrap().clone().unwrap();
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(toggle(0), true)],
                    true,
                    |_| assert_help(stack, &help, parent),
                )
                .await;
            assert!(
                control
                    .replace_options(vec![SelectOption::available(
                        OptionKey::new("added").unwrap(),
                        "Added remote"
                    )])
                    .unwrap()
            );
            assert!(help.is_active(parent));
            assert!(stack.snapshot().surface(popup).is_some());
            assert!(data.client.read().highlight().is_none());
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Esc, KeyModifiers::NONE), true)],
                    true,
                )
                .await;
            assert_eq!(stack.snapshot().top_id(), popup);
            assert_eq!(data.client.read().query().text(), "remote");
            assert!(data.client.read().highlight().is_none());
            assert_eq!(
                stack.snapshot().surface(parent).unwrap().primary_action,
                Some(ActionId::new("start").unwrap()),
                "the parent routing model is restored before a dropdown option is picked"
            );
            let map = stack.hit_regions().published().unwrap();
            let id = TargetId::new("client").unwrap().child("option:added");
            let point = map
                .targets
                .iter()
                .find(|region| region.surface == popup && region.target == id)
                .unwrap()
                .cells[0];
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(
                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            point.0,
                            point.1,
                        )),
                        true,
                    )],
                    true,
                    |_| {
                        assert_eq!(stack.snapshot().top_id(), parent);
                        assert_eq!(data.client.read().value().unwrap().as_str(), "added");
                        assert_eq!(data.starts.get(), 0);
                    },
                )
                .await;

            // A real parent dismissal retires its parked dropdown and retained updater.
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Enter, KeyModifiers::NONE), true)],
                    true,
                )
                .await;
            let parked = stack.snapshot().top_id();
            let retained = control_out.lock().unwrap().clone().unwrap();
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(toggle(0), true)],
                    true,
                    |_| assert_help(stack, &help, parent),
                )
                .await;
            let saved = data.client.read().clone();
            assert!(stack.close_top().is_some());
            assert_eq!(stack.snapshot().top_id(), SurfaceId::Screen);
            assert!(
                stack.snapshot().surface(parked).is_none(),
                "closing the parent retires the parked popup"
            );
            assert!(
                !retained
                    .replace_options(vec![SelectOption::available(
                        OptionKey::new("late").unwrap(),
                        "Late remote"
                    )])
                    .unwrap()
            );
            assert_eq!(
                *data.client.read(),
                saved,
                "late updates cannot mutate a dismissed parent"
            );
        })
        .await
        .unwrap();
    }
}
