#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, AvailabilityFn, Checked, Placement, ScopeId,
        },
        controls::{FilterField, SegmentedSwitch, TextFieldState, segmented_targets},
        focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::{SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};
    const FILTER_EDITOR_MINIMUM: u16 = 12;
    const FILTER_OUTER_MINIMUM: u16 = FILTER_EDITOR_MINIMUM + 2;
    const FILTER_ROW_GAP: u16 = 2;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn field() -> TargetId {
        TargetId::new("screen-filter").unwrap()
    }
    #[derive(Clone)]
    struct FilterFixture {
        stack: SurfaceStackHandle,
        query: State<TextFieldState>,
        selected: State<bool>,
    }
    #[derive(Props)]
    struct FilterSwitchSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
        width: u16,
        height: u16,
        handle: Arc<Mutex<Option<FilterFixture>>>,
    }
    #[component]
    fn FilterSwitchScene(
        props: &FilterSwitchSceneProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let query = hooks.use_state(TextFieldState::default);
        let selected = hooks.use_state(|| false);
        let stack =
            use_surface_stack(&mut hooks, |stack| {
                let actions = [
                    ("all", "All", "Ctrl+E", false),
                    ("local", "Local 界", "Ctrl+F", true),
                ]
                .into_iter()
                .map(|(id, title, key, is_local)| ActionDef {
                    id: action(id),
                    title: title.into(),
                    toolbar_label: None,
                    keys: vec![key.parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: Some(Checked::new(move || selected.get() == is_local)),
                    placement: Placement::Toolbar,
                    run: Handler::from(move |()| {
                        let mut selected = selected;
                        selected.set(is_local);
                    }),
                })
                .collect::<Vec<_>>();
                let mut targets = vec![TargetSpec::new(field()).with_input(
                    TextFieldState::filter_input_handler(query, Handler::default()),
                )];
                targets.extend(segmented_targets(
                    &actions,
                    GroupId::new("scope-switch").unwrap(),
                    move || {
                        stack
                            .viewport()
                            .0
                            .saturating_sub(FILTER_OUTER_MINIMUM + FILTER_ROW_GAP)
                    },
                ));
                SurfaceModel {
                    focus: FocusModel::new(targets).unwrap(),
                    actions: ActionRegistry::new(ScopeId::new("filter-switch").unwrap(), actions)
                        .unwrap(),
                    primary_action: None,
                }
            });
        *props.handle.lock().unwrap() = Some(FilterFixture {
            stack,
            query,
            selected,
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",query.read().text(),selected.get()).into_bytes())]))) {OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            View(width:100pct,column_gap:FILTER_ROW_GAP,flex_shrink:0.0_f32) {View(flex_grow:1.0_f32,min_width:FILTER_OUTER_MINIMUM) {FilterField(id:field(),value:query,placeholder:"Filter rows / 界".to_owned())} SegmentedSwitch(actions:vec![action("all"),action("local")],available_width:stack.viewport().0.saturating_sub(FILTER_OUTER_MINIMUM+FILTER_ROW_GAP))}
            Text(content:format!("Selection {}",selected.get()))
        }}})
    }
    #[tokio::test]
    async fn a_filter_keeps_twelve_editor_cells_while_the_peer_switch_compacts_or_drops_as_a_whole()
    {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for (width, height) in [(120u16, 36u16), (76, 30), (48, 24), (40, 24), (32, 24)] {
                let handle = Arc::new(Mutex::new(None));
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui =
                    element!(FilterSwitchScene(journal,width,height,handle:handle.clone()));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    driver.initial(&mut frames).await;
                    let fixture = handle.lock().unwrap().clone().unwrap();
                    let map = fixture.stack.hit_regions().published().unwrap();
                    let region = map
                        .targets
                        .iter()
                        .find(|region| {
                            region.target == field() && region.surface == SurfaceId::Screen
                        })
                        .unwrap();
                    let minimum = region
                        .cells
                        .iter()
                        .map(|(column, _)| *column)
                        .min()
                        .unwrap();
                    let maximum = region
                        .cells
                        .iter()
                        .map(|(column, _)| *column)
                        .max()
                        .unwrap();
                    assert!(
                        maximum - minimum + 1 >= 12,
                        "editor keeps room for a short query plus cursor at {width}\n{}",
                        driver.frames.last().unwrap().row_text(0)
                    );
                    let row = driver.frames.last().unwrap().row_text(0);
                    assert_eq!(
                        row.contains("All"),
                        row.contains("Local 界"),
                        "switch is complete or absent, never a clipped half-control\n{row}"
                    );
                    if row.contains("Ctrl+") {
                        assert!(
                            row.contains("Ctrl+E") && row.contains("Ctrl+F"),
                            "full keys are complete whenever shown\n{row}"
                        );
                    }
                    if width <= 40 {
                        assert!(
                            !row.contains("Ctrl+"),
                            "narrow switch is compact or absent\n{row}"
                        );
                    }
                    let visible = row.contains("All");
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::BracketedPaste("alpha-beta".into()), true)],
                            true,
                            |_| assert_eq!(fixture.query.read().text(), "alpha-beta"),
                        )
                        .await;
                    assert!(
                        driver
                            .frames
                            .last()
                            .unwrap()
                            .row_text(0)
                            .contains("alpha-beta"),
                        "short query remains readable at {width}\n{}",
                        driver.frames.last().unwrap().row_text(0)
                    );
                    let mut local = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('f'));
                    local.modifiers = KeyModifiers::CONTROL;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(local), true)],
                            true,
                            |_| {
                                assert!(
                                    fixture.selected.get(),
                                    "registered switch action stays active even if compacted"
                                )
                            },
                        )
                        .await;
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Press,
                                    KeyCode::Tab,
                                )),
                                visible,
                            )],
                            visible,
                            |_| {
                                if !visible {
                                    assert_eq!(
                                        fixture.stack.snapshot().top().focus.current(),
                                        Some(&field()),
                                        "Tab cannot land on a hidden segment"
                                    );
                                }
                            },
                        )
                        .await;
                    if width == 120 {
                        driver
                            .drive(
                                &mut frames,
                                mode,
                                vec![(TerminalEvent::Resize(32, 24), true)],
                                true,
                            )
                            .await;
                        let row = driver.frames.last().unwrap().row_text(0);
                        assert!(
                            row.contains("alpha-beta")
                                && !row.contains("All")
                                && !row.contains("Local"),
                            "live narrow resize preserves query and hides whole group\n{row}"
                        );
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![(
                                    TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Press,
                                        KeyCode::Tab,
                                    )),
                                    true,
                                )],
                                true,
                                |_| {
                                    assert_eq!(
                                        fixture.stack.snapshot().top().focus.current(),
                                        Some(&field()),
                                        "Tab leaves a newly hidden segment"
                                    )
                                },
                            )
                            .await;
                        driver
                            .drive(
                                &mut frames,
                                mode,
                                vec![(TerminalEvent::Resize(40, 24), true)],
                                true,
                            )
                            .await;
                        let row = driver.frames.last().unwrap().row_text(0);
                        assert!(
                            row.contains("All")
                                && row.contains("Local 界")
                                && !row.contains("Ctrl+"),
                            "live intermediate width shows whole compact switch\n{row}"
                        );
                        driver
                            .drive(
                                &mut frames,
                                mode,
                                vec![(TerminalEvent::Resize(120, 36), true)],
                                true,
                            )
                            .await;
                        let row = driver.frames.last().unwrap().row_text(0);
                        assert!(
                            row.contains("alpha-beta")
                                && row.contains("Ctrl+E")
                                && row.contains("Ctrl+F"),
                            "full hints return on live growth\n{row}"
                        );
                    }
                })
                .await
                .unwrap();
            }
        }
    }
}
