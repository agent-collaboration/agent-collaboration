#[cfg(test)]
mod tests {
    use crate::support::{
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
        hit_regions::HitRegionKind,
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
    };
    fn field() -> TargetId {
        TargetId::new("paged-choice").unwrap()
    }
    fn action() -> ActionId {
        ActionId::new("open-choice").unwrap()
    }
    fn key(code: KeyCode, modifiers: KeyModifiers) -> TerminalEvent {
        let mut event = KeyEvent::new(KeyEventKind::Press, code);
        event.modifiers = modifiers;
        TerminalEvent::Key(event)
    }
    #[derive(Clone)]
    struct Fixture {
        stack: SurfaceStackHandle,
        value: State<ComboBoxState>,
        picks: State<usize>,
        help: HelpControl,
    }
    #[derive(Props)]
    struct SceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        handle: Arc<Mutex<Option<Fixture>>>,
    }
    #[component]
    fn Scene(props: &SceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let value = hooks.use_state(|| {
            ComboBoxState::new(
                (0..40)
                    .map(|index| {
                        SelectOption::available(
                            OptionKey::new(format!("option{index:02}")).unwrap(),
                            format!("Option {index:02}"),
                        )
                    })
                    .collect(),
                Some(OptionKey::new("option01").unwrap()),
            )
            .unwrap()
        });
        let picks = hooks.use_state(|| 0usize);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                parent_scroll: None,
                field: field(),
                value,
                on_pick: Handler::from(move |_| {
                    let mut picks = picks;
                    picks.set(picks.get() + 1);
                }),
            });
            let open = ActionDef {
                id: action(),
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
            let help = HelpControl::new(stack, ActionId::new("help").unwrap());
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
            *props.handle.lock().unwrap() = Some(Fixture {
                stack,
                value,
                picks,
                help,
            });
            SurfaceModel {
                focus: FocusModel::new(vec![button_target(field(), &open)]).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("dropdown-pages").unwrap(),
                    vec![open, help_action],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:28u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),value.read().query().text().as_bytes().to_vec()),(stack.snapshot().top_id(),vec![u8::from(stack.hit_regions().published().is_none())])]))) {OverlayHost {View(width:35) {ComboBox(id:field(),value)}}})
    }
    #[tokio::test]
    async fn dropdown_page_keys_and_option_aliases_scroll_whole_pages_while_query_and_highlight_stay_put()
     {
        for aliases in [false, true] {
            for mode in [InputMode::Burst, InputMode::FrameSeparated] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(Scene(journal,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![
                                (key(KeyCode::Enter, KeyModifiers::NONE), true),
                                (TerminalEvent::BracketedPaste("Option".into()), true),
                            ],
                            true,
                        )
                        .await;
                    let surface = fixture.stack.snapshot().top_id();
                    let map = fixture.stack.hit_regions().published().unwrap();
                    let body = map
                        .targets
                        .iter()
                        .find(|region| {
                            region.surface == surface
                                && region.target == field().child("scroll")
                                && region.kind == HitRegionKind::ScrollArea
                        })
                        .unwrap();
                    let top = usize::from(body.cells.iter().map(|(_, row)| *row).min().unwrap());
                    let page = body
                        .cells
                        .iter()
                        .map(|(_, row)| *row)
                        .collect::<HashSet<_>>()
                        .len();
                    assert!(page > 1 && page < 20, "fixture has multiple rows and pages");
                    for (original, alias, modifiers, offset) in [
                        (KeyCode::PageDown, KeyCode::Down, KeyModifiers::ALT, page),
                        (
                            KeyCode::End,
                            KeyCode::Down,
                            KeyModifiers::ALT | KeyModifiers::SHIFT,
                            40 - page,
                        ),
                        (
                            KeyCode::PageUp,
                            KeyCode::Up,
                            KeyModifiers::ALT,
                            40 - page * 2,
                        ),
                        (
                            KeyCode::Home,
                            KeyCode::Up,
                            KeyModifiers::ALT | KeyModifiers::SHIFT,
                            0,
                        ),
                    ] {
                        let event = if aliases {
                            key(alias, modifiers)
                        } else {
                            key(original, KeyModifiers::NONE)
                        };
                        driver
                            .drive_checked(&mut frames, mode, vec![(event, true)], true, |ack| {
                                assert_eq!(
                                    ack.snapshot.0[1].1,
                                    vec![1],
                                    "paging invalidates old geometry at the input-owner callback"
                                );
                                assert_eq!(fixture.stack.snapshot().top_id(), surface);
                                assert_eq!(fixture.value.read().query().text(), "Option");
                                assert_eq!(
                                    fixture.value.read().highlight().unwrap().as_str(),
                                    "option01",
                                    "paging scrolls without picking or moving highlight"
                                );
                                assert_eq!(fixture.picks.get(), 0);
                            })
                            .await;
                        assert!(
                            driver
                                .frames
                                .last()
                                .unwrap()
                                .row_text(top)
                                .contains(&format!("Option {offset:02}")),
                            "a page moves one measured viewport, not one row: {}",
                            driver.frames.last().unwrap().row_text(top)
                        );
                    }
                    for code in [KeyCode::Up, KeyCode::Down] {
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![(key(code, KeyModifiers::CONTROL), false)],
                                false,
                                |_| {
                                    assert_eq!(
                                        fixture.value.read().highlight().unwrap().as_str(),
                                        "option01",
                                        "unbound Ctrl arrows stay inert"
                                    )
                                },
                            )
                            .await;
                    }
                    for (code, expected) in [(KeyCode::Down, "option02"), (KeyCode::Up, "option01")]
                    {
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![(key(code, KeyModifiers::NONE), true)],
                                true,
                                |_| {
                                    assert_eq!(
                                        fixture.value.read().highlight().unwrap().as_str(),
                                        expected,
                                        "plain arrows still move one option"
                                    )
                                },
                            )
                            .await;
                    }
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::Enter, KeyModifiers::NONE), true)],
                            true,
                            |_| {
                                assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
                                assert_eq!(fixture.picks.get(), 1);
                                assert_eq!(
                                    fixture.value.read().value().unwrap().as_str(),
                                    "option01"
                                );
                            },
                        )
                        .await;
                })
                .await
                .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn manually_paged_dropdown_keeps_its_scroll_query_and_highlight_after_help() {
        let handle = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(Scene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![
                        (key(KeyCode::Enter, KeyModifiers::NONE), true),
                        (TerminalEvent::BracketedPaste("Option".into()), true),
                        (key(KeyCode::PageDown, KeyModifiers::NONE), true),
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
                    vec![(key(KeyCode::F(1), KeyModifiers::NONE), true)],
                    true,
                    |_| assert!(fixture.help.is_active(fixture.stack.snapshot().top_id())),
                )
                .await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Char('7'), KeyModifiers::CONTROL), true)],
                    true,
                    |_| {
                        assert_eq!(fixture.stack.snapshot().top_id(), popup);
                        assert_eq!(*fixture.value.read(), choice);
                    },
                )
                .await;
            assert_eq!(
                driver.frames.last().unwrap(),
                &before,
                "Help returns to the exact manually paged popup"
            );
        })
        .await
        .unwrap();
    }
}
