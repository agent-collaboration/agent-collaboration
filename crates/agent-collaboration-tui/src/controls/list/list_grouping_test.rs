use crate::{
    controls::list::{ListGroup, ListGroupHeading, ListRowKey, ListRows, ListSelectionState},
    test_support::{
        event_driver::{EventAck, EventDriver, InputMode},
        grids::STANDARD_GRIDS,
        list_scene::{ListHandle, ListScene, key, rows},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;

fn selected_at(ack: &EventAck) -> &str {
    std::str::from_utf8(&ack.snapshot.0[0].1)
        .unwrap()
        .split('|')
        .next()
        .unwrap()
}
fn row(name: &str) -> ListRowKey {
    ListRowKey::new(name).unwrap()
}
fn group(name: &str, keys: &[&str]) -> ListGroup {
    ListGroup {
        heading: ListGroupHeading {
            name: name.into(),
            count: keys.len(),
            summary: None,
        },
        keys: keys.iter().map(|name| row(name)).collect(),
    }
}
fn numbered_groups() -> ListRows {
    ListRows::Grouped(
        (0..10)
            .map(|group| {
                let keys = (group * 6..group * 6 + 6)
                    .map(|index| format!("Record {index:02}"))
                    .collect::<Vec<_>>();
                self::group(
                    &format!("Group {group}"),
                    &keys.iter().map(String::as_str).collect::<Vec<_>>(),
                )
            })
            .collect(),
    )
}
#[test]
fn grouped_input_validates_atomically_and_omits_empty_groups() {
    let mut state = ListSelectionState::new(rows(&["alpha", "bravo"]), Some(row("bravo"))).unwrap();
    let before = state.clone();
    for bad in [
        ListRows::Grouped(vec![
            group("First", &["alpha"]),
            group("Second", &["alpha"]),
        ]),
        ListRows::Grouped(vec![group(" \t", &["alpha"])]),
        ListRows::Grouped(vec![ListGroup {
            heading: ListGroupHeading {
                name: "Count".into(),
                count: 2,
                summary: None,
            },
            keys: vec![row("alpha")],
        }]),
    ] {
        assert!(state.replace_rows(bad).is_err());
        assert_eq!(
            state, before,
            "invalid input leaves both ordering and selection unchanged"
        );
    }
    state
        .replace_rows(ListRows::Grouped(vec![
            group("Empty", &[]),
            group("Survives", &["bravo", "alpha"]),
        ]))
        .unwrap();
    assert_eq!(state.rows(), &[row("bravo"), row("alpha")]);
    assert_eq!(
        state.selected(),
        Some(&row("bravo")),
        "regroup preserves opaque identity"
    );
    assert_eq!(
        state.heading_before(&row("bravo")).unwrap().name,
        "Survives"
    );
    assert!(state.heading_before(&row("alpha")).is_none());
    state.replace_rows(ListRows::Grouped(vec![])).unwrap();
    assert!(state.rows().is_empty());
    assert!(state.selected().is_none());
}
#[tokio::test]
async fn grouped_headings_are_painted_but_never_record_targets_or_selections() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut first = group("Provider A", &["alpha", "bravo"]);
            first.heading.summary = Some("Ready".into());
            let input = ListRows::Grouped(vec![
                group("Empty", &[]),
                first,
                group("Provider B", &["charlie", "delta"]),
            ]);
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),initial_rows:Some(input),hover_selection:true));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                let frame = driver.frames.last().unwrap();
                assert!(
                    frame.row_text(1).contains("Provider A"),
                    "group heading is painted before its first record"
                );
                assert!(frame.row_text(1).contains("2"));
                assert!((1..frame.height).any(|line| frame.row_text(line).contains("Ready")));
                assert!(!(0..frame.height).any(|line| frame.row_text(line).contains("Empty")));
                let map = fixture.stack.hit_regions().published().unwrap();
                for key in fixture.control.rows() {
                    let target = fixture.control.row_target(&key);
                    assert!(
                        !map.targets
                            .iter()
                            .filter(|region| region.target == target)
                            .any(|region| region.cells.iter().any(|(_, y)| *y == 1)),
                        "heading is outside every record's hit wrapper"
                    );
                }
                assert_eq!(
                    fixture
                        .stack
                        .snapshot()
                        .top()
                        .focus
                        .targets()
                        .iter()
                        .filter(|target| target.can_focus())
                        .count(),
                    2,
                    "one list target and toolbar; no heading targets"
                );
                for kind in [
                    MouseEventKind::Moved,
                    MouseEventKind::Down(MouseButton::Left),
                ] {
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                    kind, 3, 1,
                                )),
                                false,
                            )],
                            false,
                            |_| {
                                assert_eq!(
                                    fixture.control.selected(),
                                    Some(row("alpha")),
                                    "heading pointer input is inert"
                                )
                            },
                        )
                        .await;
                }
                let expected = ["bravo", "charlie", "bravo", "alpha", "delta"];
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        [
                            KeyCode::Down,
                            KeyCode::Down,
                            KeyCode::Up,
                            KeyCode::Home,
                            KeyCode::End,
                        ]
                        .into_iter()
                        .map(|code| (key(code), true))
                        .collect(),
                        true,
                        |ack| {
                            let index = ack.sequence - 3;
                            assert_eq!(
                                selected_at(ack),
                                expected[index],
                                "navigation chooses records only"
                            );
                            assert_eq!(
                                std::str::from_utf8(&ack.snapshot.0[0].1)
                                    .unwrap()
                                    .split('|')
                                    .nth(1),
                                Some("list")
                            );
                        },
                    )
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(
                            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::ScrollUp,
                                3,
                                1,
                            )),
                            true,
                        )],
                        true,
                        |_| {
                            assert_eq!(
                                fixture.control.selected(),
                                Some(row("charlie")),
                                "wheel skips headings"
                            )
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
async fn grouped_pages_use_physical_viewports_and_variable_record_heights() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let input = numbered_groups();
            let heights = (0..60)
                .map(|index| if index % 2 == 0 { 2 } else { 1 })
                .collect();
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),row_heights:Some(heights),initial_rows:Some(input)));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                assert_eq!(
                    fixture.control.scroll().position().viewport_height,
                    height - 4
                );
                // Fixture positions: each group has one heading, then six records of 2/1 rows.
                // Native viewport32/26/8/20 therefore reaches record19/16/05/12 from record00.
                let destination = [
                    (120, "Record 19"),
                    (76, "Record 16"),
                    (48, "Record 05"),
                    (32, "Record 12"),
                ]
                .into_iter()
                .find_map(|(columns, record)| (width == columns).then_some(record))
                .expect("every canonical grid has an explicit physical-page expectation");
                let mut down = KeyEvent::new(KeyEventKind::Press, KeyCode::Down);
                down.modifiers = KeyModifiers::ALT;
                let mut up = KeyEvent::new(KeyEventKind::Press, KeyCode::Up);
                up.modifiers = KeyModifiers::ALT;
                let expected = [
                    destination,
                    "Record 00",
                    destination,
                    "Record 00",
                    "Record 00",
                ];
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![
                            (key(KeyCode::PageDown), true),
                            (key(KeyCode::PageUp), true),
                            (TerminalEvent::Key(down), true),
                            (TerminalEvent::Key(up), true),
                            (key(KeyCode::Enter), true),
                        ],
                        true,
                        |ack| {
                            assert_eq!(
                                selected_at(ack),
                                expected[ack.sequence - 1],
                                "a grouped page counts real heading and record rows"
                            )
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
async fn regroup_reveals_retained_selection_and_heading_without_another_input_at_all_grids() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let initial = (0..20)
                .map(|index| row(&format!("Record {index:02}")))
                .collect();
            let others = (1..20)
                .map(|index| format!("Record {index:02}"))
                .collect::<Vec<_>>();
            let grouped = ListRows::Grouped(vec![
                group(
                    "Others",
                    &others.iter().map(String::as_str).collect::<Vec<_>>(),
                ),
                group("Chosen", &["Record 00"]),
            ]);
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),row_heights:Some(vec![2;20]),initial_rows:Some(ListRows::Ungrouped(initial)),regroup_rows:Some(grouped)));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                assert_eq!(fixture.control.selected(), Some(row("Record 00")));
                let target = fixture.control.row_target(&row("Record 00"));
                let map = fixture.stack.hit_regions().published().unwrap();
                let (column, line) = map.targets.iter().find(|region|region.target==target).unwrap().cells[0];
                driver.drive(&mut frames,mode,vec![(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(MouseEventKind::Down(MouseButton::Left),column,line)),false)],false).await;
                let before = driver.frames.len();
                let acknowledgements = driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(6)), true)],
                        true,
                        |ack| assert_eq!(selected_at(ack), "Record 00", "atomic regroup retains the selected key"),
                    )
                    .await;
                assert_eq!(acknowledgements.len(),1);
                assert_eq!(acknowledgements[0].sequence,2,"only the initial pointer selection and F6 were sent");
                let first = &driver.frames[before];
                let first_text = (0..first.height)
                    .map(|line| first.row_text(line))
                    .collect::<Vec<_>>()
                    .join("\n");
                eprintln!("REGROUP_FIRST_FRAME {width}x{height} {mode:?}: selected_visible={}, heading_visible={}\n{first_text}",first_text.contains("Record 00"),first_text.contains("Chosen"));
                let settled = driver.frames.last().unwrap();
                let text = (0..settled.height).map(|line|settled.row_text(line)).collect::<Vec<_>>().join("\n");
                assert!(
                    text.contains("Record 00") && text.contains("Chosen") && text.contains('❯'),
                    "regroup settles without another input and reveals selection and its fitting heading at {width}x{height} {mode:?}\n{text}"
                );
                let map = fixture.stack.hit_regions().published().unwrap();
                let visible_rows=map.targets.iter().find(|region|region.target==target).unwrap().cells.iter().map(|(_,line)|*line).collect::<std::collections::HashSet<_>>();
                assert_eq!(visible_rows.len(),2,"the whole two-row selected record is visible");
            })
            .await
            .unwrap();
        }
    }
}

#[tokio::test]
async fn regroup_preserves_and_reveals_selection_and_pages_new_projection_before_draw() {
    for (width, height) in STANDARD_GRIDS {
        let handle = ListHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let initial = (1..=40)
            .map(|index| row(&format!("Session {index:02}")))
            .collect();
        let remaining = (1..40)
            .map(|index| format!("Session {index:02}"))
            .collect::<Vec<_>>();
        let grouped = ListRows::Grouped(vec![
            group("Last first", &["Session 40"]),
            group(
                "Remaining",
                &remaining.iter().map(String::as_str).collect::<Vec<_>>(),
            ),
        ]);
        let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),initial_rows:Some(ListRows::Ungrouped(initial)),regroup_rows:Some(grouped)));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::End), true)],
                    true,
                )
                .await;
            assert_eq!(fixture.control.selected(), Some(row("Session 40")));
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::F(6)), true), (key(KeyCode::PageDown), true)],
                    true,
                    |ack| {
                        let expected = if ack.sequence == 2 {
                            "Session 40"
                        } else {
                            "Session 01"
                        };
                        assert_eq!(
                            selected_at(ack),
                            expected,
                            "regroup then page uses new heading slots, not old geometry"
                        );
                    },
                )
                .await;
            assert!(
                (0..driver.frames.last().unwrap().height).any(|line| driver
                    .frames
                    .last()
                    .unwrap()
                    .row_text(line)
                    .contains("Session 01")),
                "new selected key is revealed"
            );
            fixture
                .control
                .replace_rows(ListRows::Grouped(vec![group(
                    "Reordered",
                    &["Session 02", "Session 01"],
                )]))
                .unwrap();
            assert_eq!(fixture.control.selected(), Some(row("Session 01")));
            driver.initial(&mut frames).await;
            assert!((0..driver.frames.last().unwrap().height).any(|line| {
                driver
                    .frames
                    .last()
                    .unwrap()
                    .row_text(line)
                    .contains("Session 01")
            }));
            fixture.control.replace_rows(rows(&["Session 02"])).unwrap();
            assert_eq!(
                fixture.control.selected(),
                Some(row("Session 02")),
                "removed selection uses prior position clamped to new last"
            );
        })
        .await
        .unwrap();
    }
}
