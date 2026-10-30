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
            FilterField, ListRow, ListRowKey, ListSelectionBinding, ListSelectionControl,
            ListSelectionState, ScrollList, TextFieldState,
        },
        focus_model::{FocusModel, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    const RECORDS: [(&str, &str); 3] = [
        ("claude-beta", "Claude Beta"),
        ("codex-alpha", "Codex Alpha"),
        ("codex-gamma", "Codex Gamma"),
    ];
    fn target(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn record_keys(query: &str) -> Vec<ListRowKey> {
        let query = query.to_lowercase();
        RECORDS
            .iter()
            .filter(|(_, label)| label.to_lowercase().contains(&query))
            .map(|(key, _)| ListRowKey::new(*key).unwrap())
            .collect()
    }
    fn record_label(key: &ListRowKey) -> &'static str {
        RECORDS
            .iter()
            .find(|(identity, _)| *identity == key.as_str())
            .unwrap()
            .1
    }
    fn definition(name: &'static str, keys: &[&str], run: Handler<()>) -> ActionDef {
        ActionDef {
            id: action(name),
            title: name.into(),
            toolbar_label: None,
            keys: keys.iter().map(|key| key.parse().unwrap()).collect(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run,
        }
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Clone)]
    struct TypeaheadFixture {
        stack: SurfaceStackHandle,
        control: ListSelectionControl,
        filter: State<TextFieldState>,
    }
    #[derive(Props)]
    struct TypeaheadSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        enabled: bool,
        handle: Arc<Mutex<Option<TypeaheadFixture>>>,
        activated: Arc<Mutex<Vec<String>>>,
    }
    #[component]
    fn TypeaheadScene(
        props: &TypeaheadSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let filter = hooks.use_state(TextFieldState::default);
        let value = hooks.use_state(|| {
            ListSelectionState::new(
                record_keys(""),
                Some(ListRowKey::new("claude-beta").unwrap()),
            )
            .unwrap()
        });
        let retained = hooks.use_ref(|| None::<ListSelectionControl>);
        let calls = hooks.use_state(|| 0usize);
        let canceled = hooks.use_state(|| 0usize);
        let activated = props.activated.clone();
        let stack = use_surface_stack(&mut hooks, |stack| {
            let editor = TextFieldState::filter_input_handler(
                filter,
                Handler::from(move |query: String| {
                    let control = retained.read().as_ref().unwrap().clone();
                    control.replace_rows(record_keys(&query)).unwrap();
                }),
            );
            let control = ListSelectionControl::new(ListSelectionBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                id: target("sessions"),
                value,
                on_select: Handler::default(),
                activation: action("open"),
                page_rows: Arc::new(|| 4),
            });
            let control = if props.enabled {
                control.with_typeahead(editor.clone())
            } else {
                control
            };
            let mut retained = retained;
            retained.set(Some(control.clone()));
            let mut targets = control.targets();
            targets.push(TargetSpec::new(target("filter")).with_input(editor));
            let focus_filter = definition(
                "focus-filter",
                &["/"],
                Handler::from(move |()| {
                    stack.focus(SurfaceId::Screen, &target("filter"));
                }),
            );
            let mut open = definition(
                "open",
                &[],
                Handler::from(move |()| {
                    activated
                        .lock()
                        .unwrap()
                        .push(value.read().selected().unwrap().as_str().to_owned());
                    let mut calls = calls;
                    calls.set(calls.get() + 1);
                }),
            );
            open.availability = AvailabilityFn::new(move || {
                if value.read().selected().is_some() {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "No matching sessions".into(),
                    }
                }
            });
            let cancel = definition(
                "cancel",
                &["Esc"],
                Handler::from(move |()| {
                    let mut canceled = canceled;
                    canceled.set(canceled.get() + 1);
                }),
            );
            let mut reserved = definition(
                "reserved",
                &["y"],
                Handler::from(|()| panic!("disabled action ran")),
            );
            reserved.availability = AvailabilityFn::new(|| Availability::Unavailable {
                reason: "Busy".into(),
            });
            SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("typeahead").unwrap(),
                    vec![focus_filter, open, cancel, reserved],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let control = retained.read().as_ref().unwrap().clone();
        *props.handle.lock().unwrap() = Some(TypeaheadFixture {
            stack,
            control: control.clone(),
            filter,
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:80u16,height:20u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,filter.read().text().as_bytes().to_vec()),(SurfaceId::Screen,stack.snapshot().top().focus.current().unwrap().as_str().as_bytes().to_vec()),(SurfaceId::Screen,canceled.get().to_le_bytes().to_vec())]))) {
            OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
                FilterField(id:target("filter"),value:filter,placeholder:"Filter sessions".to_owned())
                View(height:5) {ScrollList(id:control.scroll_target(),keyboard_target:Some(target("sessions")),scroll:control.scroll(),grow:true) {
                    #(control.rows().into_iter().map(|row|element!(ListRow(key:row.as_str().to_owned(),control:control.clone(),row:row.clone()) {Text(content:record_label(&row))}).into_any()))
                }}
                Text(content:format!("Opened {} Canceled {}",calls.get(),canceled.get()))
            }}
        })
    }
    #[tokio::test]
    async fn list_typeahead_edits_the_shared_filter_keeps_focus_and_preserves_declared_actions_and_escape_ladder()
     {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let activated = Arc::new(Mutex::new(Vec::new()));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(TypeaheadScene(journal,enabled:true,handle:handle.clone(),activated:activated.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let text = "Codex Alpha";
                let steps = text
                    .chars()
                    .map(|character| {
                        let mut event =
                            KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
                        if character.is_uppercase() {
                            event.modifiers = KeyModifiers::SHIFT;
                        }
                        (TerminalEvent::Key(event), true)
                    })
                    .collect();
                driver
                    .drive_checked(&mut frames, mode, steps, true, |ack| {
                        assert_eq!(
                            ack.snapshot.0[0].1,
                            text.chars()
                                .take(ack.sequence)
                                .collect::<String>()
                                .as_bytes(),
                            "each character edits the shared filter"
                        );
                        assert_eq!(
                            ack.snapshot.0[1].1, b"sessions",
                            "typeahead must retain list focus"
                        );
                    })
                    .await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                assert_eq!(fixture.filter.read().text(), text);
                assert_eq!(fixture.filter.read().cursor(), text.chars().count());
                assert_eq!(
                    fixture.control.rows(),
                    vec![ListRowKey::new("codex-alpha").unwrap()],
                    "the real filter callback narrows the actual list"
                );
                assert!(
                    driver.frames.last().unwrap().row_text(0).contains(text),
                    "the field displays typeahead text without focus"
                );
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
                    .await;
                assert_eq!(
                    *activated.lock().unwrap(),
                    vec!["codex-alpha"],
                    "Enter activates the top match"
                );
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Backspace), true)],
                        true,
                        |ack| {
                            assert_eq!(ack.snapshot.0[0].1, b"Codex Alph");
                            assert_eq!(ack.snapshot.0[1].1, b"sessions");
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true)],
                        true,
                        |ack| {
                            assert!(
                                ack.snapshot.0[0].1.is_empty(),
                                "Esc clears before the declared Cancel binding"
                            );
                            assert_eq!(ack.snapshot.0[2].1, 0usize.to_le_bytes());
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Esc), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[2].1,
                                1usize.to_le_bytes(),
                                "empty filter lets Esc continue through its scope"
                            )
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Char('/')), true)],
                        true,
                        |ack| {
                            assert!(
                                ack.snapshot.0[0].1.is_empty(),
                                "slash is an action, never inserted"
                            );
                            assert_eq!(ack.snapshot.0[1].1, b"filter");
                        },
                    )
                    .await;
                assert_eq!(
                    fixture.stack.snapshot().top().focus.current(),
                    Some(&target("filter"))
                );
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Char('a')), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"a",
                                "typing in the field uses the same state and callback"
                            )
                        },
                    )
                    .await;
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn list_typeahead_accepts_paste_but_never_steals_a_declared_unavailable_printable_key() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = Arc::new(Mutex::new(None));
            let activated = Arc::new(Mutex::new(Vec::new()));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(TypeaheadScene(journal,enabled:true,handle:handle.clone(),activated:activated.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::BracketedPaste("Codex Alpha".into()), true)],
                        true,
                        |ack| {
                            assert_eq!(ack.snapshot.0[0].1, b"Codex Alpha");
                            assert_eq!(ack.snapshot.0[1].1, b"sessions");
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Char('y')), false)],
                        false,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[0].1, b"Codex Alpha",
                                "unavailable declared bindings still reserve their keys"
                            )
                        },
                    )
                    .await;
                assert_eq!(
                    handle.lock().unwrap().as_ref().unwrap().control.rows(),
                    vec![ListRowKey::new("codex-alpha").unwrap()]
                );
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
                    .await;
                assert_eq!(*activated.lock().unwrap(), vec!["codex-alpha"]);
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn a_list_without_typeahead_does_not_edit_the_filter() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(TypeaheadScene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![
                        (key(KeyCode::Char('q')), false),
                        (TerminalEvent::BracketedPaste("Codex Alpha".into()), false),
                    ],
                    false,
                    |ack| {
                        assert!(ack.snapshot.0[0].1.is_empty());
                        assert_eq!(ack.snapshot.0[1].1, b"sessions");
                    },
                )
                .await;
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
