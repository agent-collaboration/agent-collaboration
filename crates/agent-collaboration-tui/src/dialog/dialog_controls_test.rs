use crate::test_support::{
    dialog_scene::DialogScene,
    event_driver::{EventDriver, InputMode},
    recording_backend::recording_terminal_render_loop,
};
use iocraft::prelude::*;
fn row(driver: &EventDriver, needle: &str) -> usize {
    (0..driver.frames.last().unwrap().height)
        .find(|row| {
            driver
                .frames
                .last()
                .unwrap()
                .row_text(*row)
                .contains(needle)
        })
        .unwrap_or_else(|| panic!("missing {needle}: {:?}", driver.frames.last()))
}
#[tokio::test]
async fn dialog_sections_labels_primary_status_and_form_height_follow_one_layout_contract() {
    for form in [false, true] {
        for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
            let (expected_width, expected_height) = match (width, height) {
                (120, 36) => (88u16, 20u16),
                (76, 30) => (57, 16),
                (48, 12) => (42, 8),
                (32, 24) => (28, 13),
                _ => panic!("standard grid has no reviewed size oracle"),
            };
            let (mut driver, config, journal) = EventDriver::new();
            let surface_stack = std::sync::Arc::new(std::sync::Mutex::new(None));
            let scroll_handle = std::sync::Arc::new(std::sync::Mutex::new(None));
            let mut ui = element!(DialogScene(journal, form, width, height,surface_stack:surface_stack.clone(),scroll_handle:scroll_handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                driver.initial(&mut frames),
            )
            .await
            .unwrap();
            let grid = driver.frames.last().unwrap();
            let top = row(&driver, "╭");
            let bottom = row(&driver, "╰");
            let border = grid.cells[top]
                .iter()
                .filter(|cell| !cell.text.trim().is_empty())
                .count();
            assert_eq!(border, usize::from(expected_width));
            assert_eq!(
                bottom - top + 1,
                if form {
                    (if width == 32 { 19usize } else { 13usize }).min(usize::from(height) - 2)
                } else {
                    usize::from(expected_height)
                },
                "form={form} {width}x{height}: {grid:?}"
            );
            assert_eq!(
                grid.cells
                    .iter()
                    .flatten()
                    .filter(|cell| cell.text == "●")
                    .count(),
                1
            );
            let title = row(&driver, "New session");
            assert!(grid.row_text(title).contains("fixture-42"));
            let actions = row(&driver, "Start");
            assert!(
                grid.row_text(actions + usize::from(width == 32))
                    .contains("Close"),
                "the phone footer wraps Close onto its next row: {width}x{height}, form={form}: {grid:?}"
            );
            assert!(
                bottom - actions
                    <= if width == 32 {
                        4 + usize::from(form)
                    } else {
                        3
                    },
                "form={form} {width}x{height} actions={actions} bottom={bottom}\n{}",
                (0..grid.height)
                    .map(|row| format!("{row}: {}", grid.row_text(row)))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            if height >= 24 && width > 32 {
                let connection = row(&driver, "Connection");
                let client = row(&driver, "Client");
                let directory = row(&driver, "Directory");
                let status = row(&driver, "Status");
                let available = row(&driver, "Available");
                assert_eq!(
                    connection,
                    title + 2,
                    "body starts below the pinned header and one gap; spare space follows the body"
                );
                assert_eq!(client, connection + 1);
                assert_eq!(directory, client + 1);
                assert_eq!(status, directory + 2);
                assert_eq!(available, status + 1);
                if form {
                    assert_eq!(
                        actions,
                        available + 2,
                        "content-sized form has exactly one blank row before actions"
                    );
                } else {
                    assert!(
                        actions >= available + 2,
                        "fixed dialog reserves the footer after the body"
                    );
                    for empty in available + 1..actions {
                        assert!(
                            grid.row_text(empty)
                                .chars()
                                .all(|cell| cell.is_whitespace() || cell == '│'),
                            "spare space follows body content"
                        );
                    }
                }
                let client_origin = grid.cells[client]
                    .iter()
                    .position(|cell| cell.text == "B")
                    .unwrap();
                let directory_origin = grid.cells[directory]
                    .iter()
                    .position(|cell| cell.text == "/")
                    .unwrap();
                assert_eq!(client_origin, directory_origin);
            }
            if width == 32 {
                let scroll = scroll_handle.lock().unwrap().clone().unwrap();
                if !form {
                    let stack = surface_stack.lock().unwrap().unwrap();
                    let published = stack.hit_regions().published().unwrap();
                    let point = *published
                        .targets
                        .iter()
                        .find(|region| region.target.as_str() == "dialog-scroll")
                        .unwrap()
                        .cells
                        .first()
                        .unwrap();
                    while scroll.position().offset < scroll.position().maximum() {
                        let before = scroll.position().offset;
                        tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            driver.drive(
                                &mut frames,
                                InputMode::FrameSeparated,
                                vec![(
                                    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                        MouseEventKind::ScrollDown,
                                        point.0,
                                        point.1,
                                    )),
                                    true,
                                )],
                                true,
                            ),
                        )
                        .await
                        .unwrap();
                        assert_eq!(
                            scroll.position().offset,
                            before + 1,
                            "one wheel event advances the measured phone body"
                        );
                    }
                }
                let end = driver.frames.last().unwrap().round_frame_text();
                for fact in ["Status", "Available", "Start", "Close"] {
                    assert!(
                        end.contains(fact),
                        "phone body remains reachable: {fact}, {:?}\n{end}",
                        scroll.position()
                    );
                }
            }
        }
    }
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn ctrl_enter() -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}
#[tokio::test]
async fn real_dialog_fields_route_keys_and_ctrl_enter_cancels_its_dropdown_before_start_once() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(DialogScene(journal,form:true,width:76u16,height:30u16));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let edits = driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::Down), true),
                        (key(KeyCode::Left), true),
                        (TerminalEvent::BracketedPaste("X".into()), true),
                        (key(KeyCode::Up), true),
                        (key(KeyCode::Enter), true),
                        (key(KeyCode::Down), true),
                        (ctrl_enter(), true),
                        (ctrl_enter(), false),
                    ],
                    true,
                )
                .await;
            let last = String::from_utf8(edits.last().unwrap().snapshot.0[1].1.clone()).unwrap();
            assert!(last.starts_with("/work/repXo|10|beta|1|"), "{last}");
            assert_eq!(edits.last().unwrap().snapshot.0.len(), 2);
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                .await;
            assert!(
                !driver
                    .frames
                    .last()
                    .unwrap()
                    .cells
                    .iter()
                    .enumerate()
                    .any(|(row, _)| {
                        driver
                            .frames
                            .last()
                            .unwrap()
                            .row_text(row)
                            .contains("New session")
                    })
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn primary_button_is_reachable_by_tab_enter_shift_tab_and_pointer() {
    for pointer in [false, true] {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(DialogScene(journal,form:true,width:76u16,height:30u16));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let activation = if pointer {
                let grid = driver.frames.last().unwrap();
                let row = (0..grid.height)
                    .find(|row| grid.row_text(*row).contains('●'))
                    .unwrap();
                let column = grid.cells[row]
                    .iter()
                    .position(|cell| cell.text == "S")
                    .unwrap();
                TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::Down(MouseButton::Left),
                    u16::try_from(column).unwrap(),
                    u16::try_from(row).unwrap(),
                ))
            } else {
                let previous = driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![
                            (key(KeyCode::Tab), true),
                            (key(KeyCode::Tab), true),
                            (key(KeyCode::BackTab), true),
                        ],
                        true,
                    )
                    .await;
                assert!(
                    String::from_utf8_lossy(&previous.last().unwrap().snapshot.0[1].1)
                        .ends_with("|directory")
                );
                driver
                    .drive(
                        &mut frames,
                        InputMode::Burst,
                        vec![(key(KeyCode::Tab), true)],
                        true,
                    )
                    .await;
                key(KeyCode::Enter)
            };
            let result = driver
                .drive_checked(
                    &mut frames,
                    InputMode::Burst,
                    vec![(activation, true)],
                    true,
                    |ack| {
                        assert!(String::from_utf8_lossy(&ack.snapshot.0[1].1).contains("|beta|1|"))
                    },
                )
                .await;
            assert!(
                String::from_utf8_lossy(&result.last().unwrap().snapshot.0[1].1)
                    .contains("|beta|1|")
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

fn assert_primary_button_palette(
    driver: &EventDriver,
    stack: crate::surfaces::surface_handle::SurfaceStackHandle,
    expected: Color,
) {
    let frame = driver.frames.last().unwrap();
    let target = crate::surfaces::focus_model::TargetId::for_action(
        &crate::test_support::dialog_scene::action("start"),
    );
    let map = stack.hit_regions().published().unwrap();
    let surface = stack.snapshot().top_id();
    let region = map
        .targets
        .iter()
        .find(|region| region.surface == surface && region.target == target)
        .unwrap();
    let labels = region
        .cells
        .iter()
        .copied()
        .filter(|(column, row)| {
            (0..5usize)
                .filter_map(|offset| frame.cell(usize::from(*column) + offset, usize::from(*row)))
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                == "Start"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        labels.len(),
        1,
        "only the actual primary button, not its key-hint copy"
    );
    let (column, row) = labels[0];
    for offset in 0..5usize {
        let cell = frame
            .cell(usize::from(column) + offset, usize::from(row))
            .unwrap();
        assert!(
            region
                .cells
                .contains(&(column + u16::try_from(offset).unwrap(), row))
        );
        assert_eq!(cell.style.foreground, Some(expected));
        assert!(
            !cell.style.bold,
            "an unfocused, unchecked primary label has normal weight"
        );
        assert!(
            !cell.style.underline,
            "the primary mark must not invent focus"
        );
        assert_eq!(
            cell.style.background,
            Some(Color::Rgb {
                r: 39,
                g: 44,
                b: 52
            })
        );
    }
    let marks = region
        .cells
        .iter()
        .filter_map(|(column, row)| frame.cell(usize::from(*column), usize::from(*row)))
        .filter(|cell| cell.text == "●")
        .collect::<Vec<_>>();
    assert_eq!(marks.len(), 1, "the declared primary has exactly one mark");
    assert_eq!(
        marks[0].style.foreground,
        Some(Color::Rgb {
            r: 156,
            g: 188,
            b: 251
        })
    );
}

#[tokio::test]
async fn marked_primary_and_unavailable_footer_button_use_their_actual_contract_palette() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(DialogScene(journal,form:true,width,height,surface_stack:observed.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let stack = observed.lock().unwrap().unwrap();
            assert_primary_button_palette(
                &driver,
                stack,
                Color::Rgb {
                    r: 156,
                    g: 188,
                    b: 251,
                },
            );
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(ctrl_enter(), true)],
                    true,
                    |ack| {
                        assert!(String::from_utf8_lossy(&ack.snapshot.0[1].1).contains("|beta|1|"));
                    },
                )
                .await;
            assert_primary_button_palette(
                &driver,
                stack,
                Color::Rgb {
                    r: 166,
                    g: 175,
                    b: 189,
                },
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
