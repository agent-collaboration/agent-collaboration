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
        controls::{
            ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, ComboTransition, OptionKey,
            SelectOption,
        },
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    fn key(value: &str) -> OptionKey {
        OptionKey::new(value).unwrap()
    }
    fn event(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    fn options() -> Vec<SelectOption> {
        let mut values = vec![
            SelectOption::available(key("alpha"), "Alpha local"),
            SelectOption::available(key("beta"), "Beta remote"),
            SelectOption::available(key("gamma"), "Gamma local"),
        ];
        values.push(SelectOption {
            key: key("offline"),
            label: "Unavailable".into(),
            secondary: None,
            availability: Availability::Unavailable {
                reason: "Offline".into(),
            },
        });
        values
    }
    #[test]
    fn own_typing_and_external_updates_keep_different_keyed_highlight_rules() {
        let mut state = ComboBoxState::new(options(), Some(key("beta"))).unwrap();
        state.begin();
        state.handle_event(&event(KeyCode::Char('a')));
        assert_eq!(
            state.highlight(),
            Some(&key("beta")),
            "still-matching typing must keep the current highlight"
        );
        state.handle_event(&event(KeyCode::Down));
        assert_eq!(state.highlight(), Some(&key("gamma")));
        let mut reordered = options();
        reordered.reverse();
        state.replace_options(reordered).unwrap();
        assert_eq!(
            state.highlight(),
            Some(&key("gamma")),
            "highlight follows identity through reorder"
        );
        let without_gamma = options()
            .into_iter()
            .filter(|option| option.key != key("gamma"))
            .collect();
        state.replace_options(without_gamma).unwrap();
        assert_eq!(state.highlight(), None);
        assert_eq!(
            state.handle_event(&event(KeyCode::Enter)),
            ComboTransition::Changed
        );
        assert_eq!(state.value(), Some(&key("beta")));
        state.handle_event(&event(KeyCode::Backspace));
        assert_eq!(
            state.highlight(),
            None,
            "typing after external removal must not silently select a replacement"
        );
        assert_eq!(
            state.handle_event(&event(KeyCode::Enter)),
            ComboTransition::Changed
        );
        state.handle_event(&event(KeyCode::Down));
        assert_eq!(state.highlight(), Some(&key("alpha")));
        state.handle_event(&event(KeyCode::Char('l')));
        assert_eq!(state.highlight(), Some(&key("alpha")));
        state.handle_event(&event(KeyCode::Char('z')));
        assert!(state.matching().is_empty());
        assert_eq!(state.highlight(), None);
        let mut unavailable = ComboBoxState::new(options(), Some(key("offline"))).unwrap();
        unavailable.begin();
        assert_eq!(unavailable.highlight(), Some(&key("offline")));
        assert_eq!(unavailable.pick(), None);
        assert!(
            ComboBoxState::new(vec![options()[0].clone(), options()[0].clone()], None).is_err()
        );
    }
    #[derive(Clone, Default)]
    struct ModelHandle(Arc<Mutex<Option<ComboBoxControl>>>);
    #[derive(Props)]
    struct ComboSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: ModelHandle,
    }
    fn field() -> TargetId {
        TargetId::new("client").unwrap()
    }
    fn action() -> ActionId {
        ActionId::new("open").unwrap()
    }
    #[component]
    fn ComboScene(props: &ComboSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| ComboBoxState::new(options(), Some(key("beta"))).unwrap());
        let picks = hooks.use_state(|| 0usize);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                field: field(),
                value,
                on_pick: Handler::from(move |_| {
                    let mut picks = picks;
                    picks.set(picks.get() + 1);
                }),
            });
            *props.handle.0.lock().unwrap() = Some(control.clone());
            let open = ActionDef {
                id: action(),
                title: "Client",
                keys: vec!["F3".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Toolbar,
                run: Handler::from(move |()| {
                    control
                        .open(OverlayLayout::AtCells {
                            left: 2,
                            top: 3,
                            width: 35,
                            height: 8,
                        })
                        .unwrap();
                }),
            };
            SurfaceModel {
                focus: FocusModel::new(vec![TargetSpec::new(field()).with_activation(action())])
                    .unwrap(),
                actions: ActionRegistry::new(ScopeId::new("combo").unwrap(), vec![open]).unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:55u16,height:15u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|{
            let state=value.read().clone();let mut surfaces=vec![(SurfaceId::Screen,format!("{}|{}",state.value().map_or("-",OptionKey::as_str),picks.get()).into_bytes())];
            if stack.snapshot().top_id()!=SurfaceId::Screen {surfaces.push((stack.snapshot().top_id(),format!("{}|{}|{}",state.query().text(),state.highlight().map_or("-",OptionKey::as_str),stack.snapshot().top().focus.current().map_or("-",TargetId::as_str)).into_bytes()));}
            journal.processed(SurfaceSnapshot(surfaces));
        }) {OverlayHost {View(width:30,height:1) {ComboBox(id:field(),value)}}})
    }
    fn dropdown(query: &str, highlight: &str) -> Vec<u8> {
        format!("{query}|{highlight}|{}", field().child("filter").as_str()).into_bytes()
    }
    #[tokio::test]
    async fn tab_stays_on_the_filter_and_enter_picks_its_keyed_highlight() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ComboScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let acks = driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![
                            (event(KeyCode::Enter), true),
                            (event(KeyCode::Tab), false),
                            (event(KeyCode::Enter), true),
                        ],
                        false,
                        |ack| {
                            if ack.sequence == 2 {
                                assert_eq!(
                                    ack.snapshot.0[1].1,
                                    dropdown("", "beta"),
                                    "Tab must not land on a pointer row: {mode:?}"
                                );
                            }
                        },
                    )
                    .await;
                assert_eq!(
                    acks.last().unwrap().snapshot.0,
                    [(SurfaceId::Screen, b"beta|1".to_vec())]
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn dropdown_filters_picks_cancels_and_excludes_unavailable_values_in_both_modes() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ComboScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let acks = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (event(KeyCode::Enter), true),
                            (event(KeyCode::Char('a')), true),
                            (event(KeyCode::Enter), true),
                        ],
                        false,
                    )
                    .await;
                assert_eq!(acks[1].snapshot.0[0].1, b"beta|0");
                assert_eq!(acks[1].snapshot.0[1].1, dropdown("a", "beta"));
                assert_eq!(
                    acks.last().unwrap().snapshot.0,
                    [(SurfaceId::Screen, b"beta|1".to_vec())]
                );
                let cancelled = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (event(KeyCode::F(3)), true),
                            (event(KeyCode::Down), true),
                            (event(KeyCode::Esc), true),
                        ],
                        false,
                    )
                    .await;
                assert_eq!(cancelled.last().unwrap().snapshot.0[0].1, b"beta|1");
                let blocked = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (event(KeyCode::F(3)), true),
                            (event(KeyCode::Down), true),
                            (event(KeyCode::Down), true),
                            (event(KeyCode::Enter), false),
                        ],
                        true,
                    )
                    .await;
                assert_eq!(blocked.last().unwrap().snapshot.0[0].1, b"beta|1");
                assert_eq!(
                    blocked.last().unwrap().snapshot.0[1].1,
                    dropdown("", "offline")
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::BracketedPaste("zzzz".into()), true)],
                        true,
                    )
                    .await;
                assert!((0..driver.frames.last().unwrap().cells.len()).any(|row| {
                    driver
                        .frames
                        .last()
                        .unwrap()
                        .row_text(row)
                        .contains("No match")
                }));
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn external_removal_makes_enter_inert_until_arrow_navigation() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = ModelHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ComboScene(journal,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(event(KeyCode::Enter), true)], true)
                    .await;
                handle
                    .0
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .replace_options(
                        options()
                            .into_iter()
                            .filter(|option| option.key != key("beta"))
                            .collect(),
                    )
                    .unwrap();
                let ignored = driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(event(KeyCode::Enter), false)],
                        true,
                        |ack| {
                            assert_eq!(ack.snapshot.0[0].1, b"beta|0");
                            assert_eq!(ack.snapshot.0[1].1, dropdown("", "-"));
                        },
                    )
                    .await;
                assert_eq!(ignored.len(), 1);
                let chosen = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(event(KeyCode::Down), true), (event(KeyCode::Enter), true)],
                        true,
                    )
                    .await;
                assert_eq!(chosen.last().unwrap().snapshot.0[0].1, b"alpha|1");
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }

    async fn pick_after_update(updated: Vec<SelectOption>, label: &str, expected: &[u8]) {
        let handle = ModelHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ComboScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(event(KeyCode::Enter), true)],
                    true,
                )
                .await;
            assert!(
                handle
                    .0
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .replace_options(updated)
                    .unwrap()
            );
            driver.initial(&mut frames).await;
            let grid = driver.frames.last().unwrap();
            let row = (0..grid.height)
                .find(|row| grid.row_text(*row).contains(label))
                .unwrap();
            let column = grid.row_text(row).find(label).unwrap();
            let click = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Left),
                u16::try_from(column).unwrap(),
                u16::try_from(row).unwrap(),
            ));
            let picked = driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(click, true)],
                    true,
                    |ack| assert_eq!(ack.snapshot.0, [(SurfaceId::Screen, expected.to_vec())]),
                )
                .await;
            assert_eq!(picked.len(), 1);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn option_added_to_an_open_dropdown_is_clickable() {
        let mut updated = options();
        updated.insert(0, SelectOption::available(key("delta"), "Delta added"));
        pick_after_update(updated, "Delta added", b"delta|1").await;
    }

    #[tokio::test]
    async fn click_after_reorder_and_removal_picks_the_row_under_the_pointer() {
        let mut updated = options();
        updated.retain(|option| option.key != key("alpha"));
        updated.reverse();
        pick_after_update(updated, "Gamma local", b"gamma|1").await;
    }

    #[tokio::test]
    async fn late_option_update_does_not_mutate_a_closed_parent_draft() {
        use crate::support::dialog_scene::DialogScene;
        let combo = Arc::new(Mutex::new(None));
        let data = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(DialogScene(journal,form:true,width:76u16,height:30u16,combo:combo.clone(),data:data.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![
                        (event(KeyCode::Enter), true),
                        (event(KeyCode::Esc), true),
                        (event(KeyCode::Esc), true),
                    ],
                    true,
                )
                .await;
            let value = data.lock().unwrap().unwrap().client;
            let before = value.read().clone();
            assert!(
                !combo
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .replace_options(vec![SelectOption::available(key("late"), "Late update")])
                    .unwrap()
            );
            assert_eq!(
                *value.read(),
                before,
                "a retained control must respect the actual parent lifetime"
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
