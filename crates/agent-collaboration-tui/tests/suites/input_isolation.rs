#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, InputMode},
        input_scene::{Scene, SceneHandle},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::AvailabilityFn, surface_stack::SurfaceId, theme::SurfaceRole,
    };
    use iocraft::prelude::*;
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    fn click(column: u16, row: u16) -> TerminalEvent {
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        ))
    }

    #[tokio::test]
    async fn tab_group_arrows_and_click_focus_the_same_target_that_enter_activates() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for navigation in [
                key(KeyCode::Tab),
                key(KeyCode::BackTab),
                key(KeyCode::Down),
                key(KeyCode::Up),
                key(KeyCode::Left),
                key(KeyCode::Right),
                click(3, 6),
            ] {
                let (mut driver, config, journal) = EventDriver::new();
                let mut ui = element!(Scene(journal));
                let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                let scenario = async {
                    driver.initial(&mut frames).await;
                    let acknowledgements = driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(navigation.clone(), true), (key(KeyCode::Enter), true)],
                            true,
                            |ack| {
                                assert!(
                                    String::from_utf8_lossy(&ack.snapshot.0[0].1)
                                        .ends_with("|other"),
                                    "{navigation:?} / {mode:?}"
                                );
                                if ack.sequence == 2 {
                                    assert_eq!(
                                        ack.handlers.last().unwrap(),
                                        "other",
                                        "{navigation:?} / {mode:?}"
                                    );
                                }
                            },
                        )
                        .await;
                    assert_eq!(
                        acknowledgements.last().unwrap().handlers.last().unwrap(),
                        "other"
                    );
                    let focused=crate::support::frame_grid::FrameGrid::from_canvas(&element!(Text(content:"x",color:agent_collaboration_tui::theme::Theme::default().foreground(agent_collaboration_tui::theme::TextRole::Focused))).render(None)).unwrap().cell(0,0).unwrap().style.foreground;
                    let grid = driver.frames.last().unwrap();
                    assert_eq!(grid.cell(2, 6).unwrap().style.foreground, focused);
                    assert_ne!(grid.cell(2, 3).unwrap().style.foreground, focused);
                    let back = driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(key(KeyCode::BackTab), true), (key(KeyCode::Enter), true)],
                            true,
                        )
                        .await;
                    assert!(
                        String::from_utf8_lossy(&back.last().unwrap().snapshot.0[0].1)
                            .ends_with("|filter")
                    );
                    assert_eq!(back.last().unwrap().handlers.last().unwrap(), "details");
                };
                tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                    .await
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    async fn opening_text_paste_arrows_and_escape_enter_respect_event_time_surface_in_both_modes() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(Scene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let events = vec![
                    (key(KeyCode::F(10)), true),
                    (key(KeyCode::Char('a')), false),
                    (TerminalEvent::BracketedPaste("paste".into()), false),
                    (key(KeyCode::Down), true),
                    (key(KeyCode::Esc), true),
                    (key(KeyCode::Enter), true),
                ];
                let acknowledgements = driver.drive(&mut frames, mode, events, true).await;
                for ack in acknowledgements.iter().take(5) {
                    assert_eq!(ack.snapshot.0[0].1, b"seed|0|0|filter", "{mode:?}: {ack:?}");
                }
                let last = acknowledgements.last().unwrap();
                assert_eq!(
                    last.snapshot.0,
                    [(SurfaceId::Screen, b"seed|1|0|filter".to_vec())]
                );
                assert_eq!(last.handlers, ["menu", "details"]);
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn outside_menu_click_is_consumed_dialog_click_ignored_and_first_frame_click_ignored() {
        for (opening, should_close) in [(KeyCode::F(10), true), (KeyCode::F(2), false)] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(Scene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let opened = driver
                    .drive(
                        &mut frames,
                        InputMode::Burst,
                        vec![(key(opening), true), (click(3, 3), false)],
                        true,
                    )
                    .await;
                assert_eq!(
                    opened.last().unwrap().snapshot.0.len(),
                    2,
                    "click before the first overlay frame is ignored"
                );
                let clicked = driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(click(3, 3), should_close)],
                        should_close,
                    )
                    .await;
                let last = clicked.last().unwrap();
                assert_eq!(last.snapshot.0[0].1, b"seed|0|0|filter");
                assert_eq!(last.snapshot.0.len(), if should_close { 1 } else { 2 });
                assert!(!last.handlers.iter().any(|handler| handler == "details"));
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn serialized_overlay_preserves_corners_border_and_background_with_a_wide_left_neighbor_on_every_row()
     {
        for glyph in ["📂", "✉️", "界"] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(Scene(journal,wide_glyph:glyph.to_owned()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::F(10)), true)],
                        true,
                    )
                    .await;
                let grid = driver.frames.last().unwrap();
                for row in 4..10 {
                    let expected = if row == 4 {
                        "┌"
                    } else if row == 9 {
                        "└"
                    } else {
                        "│"
                    };
                    assert_eq!(
                        grid.cell(20, row).unwrap().text,
                        expected,
                        "{glyph} row {row}: {}",
                        grid.row_text(row)
                    );
                    assert_eq!(
                        grid.cell(19, row).unwrap().style.background,
                        Some(context_overlay_color())
                    );
                    assert_eq!(grid.cell(19, row).unwrap().text, " ");
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    fn context_overlay_color() -> Color {
        agent_collaboration_tui::theme::Theme::default().background(SurfaceRole::Raised)
    }

    #[tokio::test]
    async fn an_overlay_at_column_zero_keeps_its_complete_left_edge() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(Scene(journal,overlay_left:Some(0)));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(10)), true)],
                    true,
                )
                .await;
            let grid = driver.frames.last().unwrap();
            for row in 4..10 {
                assert_eq!(
                    grid.cell(0, row).unwrap().text,
                    if row == 4 {
                        "┌"
                    } else if row == 9 {
                        "└"
                    } else {
                        "│"
                    }
                );
                assert_eq!(
                    grid.cell(0, row).unwrap().style.background,
                    Some(context_overlay_color())
                );
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn covered_foregrounds_and_backgrounds_dim_outside_the_footprint_and_restore_in_the_closing_frame()
     {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(Scene(journal));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let normal = driver.frames.last().unwrap().clone();
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(10)), true)],
                    true,
                )
                .await;
            let covered = driver.frames.last().unwrap();
            for column in (0..20).step_by(2) {
                let before = normal.cell(column, 11).unwrap();
                let after = covered.cell(column, 11).unwrap();
                assert_eq!(before.text, "x");
                assert_eq!(before.text, after.text);
                assert_ne!(before.style.foreground, after.style.foreground);
            }
            for column in (0..12).step_by(2) {
                let before = normal.cell(column, 13).unwrap();
                let after = covered.cell(column, 13).unwrap();
                assert_eq!(before.text, "b");
                assert_eq!(before.text, after.text);
                assert_ne!(before.style.background, after.style.background);
            }
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::Esc), true)],
                    true,
                )
                .await;
            assert_eq!(*driver.frames.last().unwrap(), normal);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn closing_by_key_or_outside_click_restores_identity_after_reorder_removal_and_disable() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            for outside_click in [false, true] {
                for replacement in ["reordered", "removed", "disabled", "empty"] {
                    let handle = SceneHandle::default();
                    let (mut driver, config, journal) = EventDriver::new();
                    let mut ui = element!(Scene(journal,handle:handle.clone()));
                    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
                    let scenario = async {
                        driver.initial(&mut frames).await;
                        driver
                            .drive(
                                &mut frames,
                                InputMode::FrameSeparated,
                                vec![(key(KeyCode::Tab), true), (key(KeyCode::F(10)), true)],
                                true,
                            )
                            .await;
                        let stack = handle.get();
                        let old = stack
                            .snapshot()
                            .surface(SurfaceId::Screen)
                            .unwrap()
                            .focus
                            .targets()
                            .to_vec();
                        let filter = old
                            .iter()
                            .find(|target| target.id.as_str() == "filter")
                            .unwrap()
                            .clone();
                        let mut other = old
                            .iter()
                            .find(|target| target.id.as_str() == "other")
                            .unwrap()
                            .clone();
                        let targets = match replacement {
                            "reordered" => vec![other, filter],
                            "removed" => vec![filter],
                            "disabled" => {
                                other.availability = AvailabilityFn::new(|| {
                                    agent_collaboration_tui::actions::Availability::Unavailable {
                                        reason: "removed by update".into(),
                                    }
                                });
                                vec![other, filter]
                            }
                            _ => Vec::new(),
                        };
                        assert!(
                            stack
                                .replace_targets_if_open(SurfaceId::Screen, targets)
                                .unwrap()
                        );
                        let close = if outside_click {
                            click(3, 3)
                        } else {
                            key(KeyCode::Esc)
                        };
                        let acknowledgements = driver
                            .drive(&mut frames, mode, vec![(close, true)], true)
                            .await;
                        let expected = match replacement {
                            "reordered" | "disabled" => b"seed|0|0|other".as_slice(),
                            "empty" => b"seed|0|0|-".as_slice(),
                            _ => b"seed|0|0|filter".as_slice(),
                        };
                        assert_eq!(
                            acknowledgements[0].snapshot.0,
                            [(SurfaceId::Screen, expected.to_vec())],
                            "{replacement} / {outside_click} / {mode:?}"
                        );
                        assert!(
                            !acknowledgements[0]
                                .handlers
                                .iter()
                                .any(|handler| handler == "details")
                        );
                    };
                    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                        .await
                        .unwrap();
                }
            }
        }
    }

    #[tokio::test]
    async fn release_and_repeat_never_trigger_effects_while_character_repeats_edit_text() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(Scene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario =
                async {
                    driver.initial(&mut frames).await;
                    let mut control = KeyEvent::new(KeyEventKind::Repeat, KeyCode::Char('c'));
                    control.modifiers = KeyModifiers::CONTROL;
                    let repeat_quit = TerminalEvent::Key(control.clone());
                    control.kind = KeyEventKind::Release;
                    let release_quit = TerminalEvent::Key(control.clone());
                    let ignored = driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![
                                (repeat_quit, false),
                                (release_quit, false),
                                (
                                    TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Repeat,
                                        KeyCode::Enter,
                                    )),
                                    false,
                                ),
                                (
                                    TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Release,
                                        KeyCode::Enter,
                                    )),
                                    false,
                                ),
                                (
                                    TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Repeat,
                                        KeyCode::F(10),
                                    )),
                                    false,
                                ),
                            ],
                            false,
                        )
                        .await;
                    assert!(
                        ignored.iter().all(|ack| ack.snapshot.0
                            == [(SurfaceId::Screen, b"seed|0|0|filter".to_vec())]),
                        "{ignored:?}"
                    );
                    let edited = driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::Key(KeyEvent::new(
                                    KeyEventKind::Repeat,
                                    KeyCode::Char('a'),
                                )),
                                true,
                            )],
                            true,
                        )
                        .await;
                    assert_eq!(edited[0].snapshot.0[0].1, b"seeda|0|0|filter");
                    control.kind = KeyEventKind::Press;
                    let quit = driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(control), true)],
                            true,
                        )
                        .await;
                    assert_eq!(quit[0].snapshot.0[0].1, b"seeda|0|1|filter");
                };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
