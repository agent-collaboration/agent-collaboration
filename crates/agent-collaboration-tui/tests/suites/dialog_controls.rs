#[cfg(test)]
mod tests {
    use crate::support::{
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
            for (width, height, expected_width, expected_height) in [
                (120u16, 36u16, 88u16, 20u16),
                (76, 30, 57, 16),
                (48, 24, 42, 13),
                (48, 12, 42, 8),
            ] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(DialogScene(journal, form, width, height));
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
                        13usize.min(usize::from(height) - 2)
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
                assert!(grid.row_text(actions).contains("Close"));
                assert!(
                    bottom - actions <= 3,
                    "form={form} {width}x{height} actions={actions} bottom={bottom}\n{}",
                    (0..grid.height)
                        .map(|row| format!("{row}: {}", grid.row_text(row)))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                if height >= 24 {
                    let connection = row(&driver, "Connection");
                    let client = row(&driver, "Client");
                    let directory = row(&driver, "Directory");
                    let status = row(&driver, "Status");
                    let available = row(&driver, "Available");
                    if form {
                        assert_eq!(
                            connection,
                            title + 2,
                            "one blank row separates pinned context from the form body"
                        );
                    }
                    assert_eq!(client, connection + 1);
                    assert_eq!(directory, client + 1);
                    assert_eq!(status, directory + 2);
                    assert_eq!(available, status + 1);
                    assert_eq!(
                        actions,
                        available + 2,
                        "exactly one blank row before actions"
                    );
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
            let scenario =
                async {
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
                    let last =
                        String::from_utf8(edits.last().unwrap().snapshot.0[1].1.clone()).unwrap();
                    assert!(last.starts_with("/work/repXo|10|beta|1|"), "{last}");
                    assert_eq!(edits.last().unwrap().snapshot.0.len(), 2);
                    driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                        .await;
                    assert!(!driver.frames.last().unwrap().cells.iter().enumerate().any(
                        |(row, _)| {
                            driver
                                .frames
                                .last()
                                .unwrap()
                                .row_text(row)
                                .contains("New session")
                        }
                    ));
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
                            assert!(
                                String::from_utf8_lossy(&ack.snapshot.0[1].1).contains("|beta|1|")
                            )
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
}
