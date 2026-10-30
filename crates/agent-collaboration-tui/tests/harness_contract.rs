#[cfg(test)]
mod support;
#[cfg(test)]
mod tests {
    use super::support;
    use agent_collaboration_tui::surface_stack::SurfaceId;
    use iocraft::prelude::*;
    use support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::{FrameError, FrameGrid},
    };

    #[test]
    fn serialized_frame_reader_rejects_unknown_csi_sequences() {
        assert!(matches!(
            FrameGrid::new(12, 1).apply("hello\x1b[2J"),
            Err(FrameError::UnknownEscape(_))
        ));
    }
    #[test]
    fn serialized_frame_reader_rejects_osc8_hyperlinks() {
        assert!(matches!(
            FrameGrid::new(12, 1).apply("\x1b]8;;https://example.invalid\x1b\\link"),
            Err(FrameError::UnknownEscape(_))
        ));
    }
    #[test]
    fn serialized_frame_reader_erases_to_the_known_row_end() {
        let mut grid = FrameGrid::new(8, 1);
        grid.apply("old text").unwrap();
        grid.apply("\x1b[48;2;4;5;6mnew\x1b[K").unwrap();
        assert_eq!(grid.row_text(0), "new     ");
        for column in 3..8 {
            let cell = grid.cell(column, 0).unwrap();
            assert!(cell.text.is_empty());
            assert_eq!(cell.style.background, Some(Color::Rgb { r: 4, g: 5, b: 6 }));
        }
    }
    #[test]
    fn serialized_frame_reader_tracks_real_styles_wide_text_and_resets() {
        let canvas =
        element!(View(width: 20, height: 2, background_color: Color::Rgb { r: 4, g: 5, b: 6 }) {
            Text(content: "📂✉️界", color: Color::White, weight: Weight::Bold)
        })
        .render(None);
        let grid = FrameGrid::from_canvas(&canvas).unwrap();
        let mut serialized = Vec::new();
        canvas.write_ansi(&mut serialized).unwrap();
        for (column, glyph) in [(0, "📂"), (2, "✉️"), (4, "界")] {
            assert_eq!(grid.cell(column, 0).unwrap().text, glyph);
            assert!(grid.cell(column + 1, 0).unwrap().continuation);
            assert!(
                grid.cell(column, 0).unwrap().style.bold,
                "column {column}; ANSI: {:?}",
                String::from_utf8_lossy(&serialized)
            );
            assert_eq!(
                grid.cell(column, 0).unwrap().style.foreground,
                Some(Color::AnsiValue(15))
            );
        }
        assert!(
            grid.cells
                .iter()
                .flatten()
                .all(|cell| cell.style.background == Some(Color::Rgb { r: 4, g: 5, b: 6 }))
        );
        assert!(FrameGrid::new(2, 1).apply("\x1b[38;2;999;0;0mX").is_err());
    }

    #[derive(Props)]
    struct HarnessProbeProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn HarnessProbe(props: &HarnessProbeProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let mut count = hooks.use_state(|| 0usize);
        let mut quit = hooks.use_state(|| false);
        let mut system = hooks.use_context_mut::<SystemContext>();
        hooks.use_terminal_events({
            let journal = props.journal.clone();
            move |event| {
                match event {
                    TerminalEvent::Key(key) if key.code == KeyCode::Char('a') => {
                        count.set(count.get() + 1);
                        journal.log_handler("increment");
                    }
                    TerminalEvent::Key(key)
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        quit.set(true)
                    }
                    _ => {}
                }
                journal.processed(SurfaceSnapshot(vec![(
                    SurfaceId::Screen,
                    count.get().to_le_bytes().to_vec(),
                )]));
            }
        });
        if quit.get() {
            system.exit();
        }
        element!(View(width: 20, height: 2) { Text(content: format!("Count {}", count.get())) })
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }

    #[tokio::test]
    async fn burst_driver_delivers_all_events_before_any_redraw() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HarnessProbe(journal: journal.clone()));
        let mut frames = Box::pin(ui.mock_terminal_render_loop(config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let initial_frames = journal.draw_count();
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::Char('a')), true); 3],
                    true,
                )
                .await;
            assert_eq!(acknowledgements.len(), 3);
            assert!(
                acknowledgements
                    .iter()
                    .all(|ack| ack.drawn_frames == initial_frames),
                "{acknowledgements:?}"
            );
            assert_eq!(
                acknowledgements.last().unwrap().snapshot,
                SurfaceSnapshot(vec![(SurfaceId::Screen, 3usize.to_le_bytes().to_vec())])
            );
            assert_eq!(
                acknowledgements.last().unwrap().handlers,
                ["increment", "increment", "increment"]
            );
            assert_eq!(driver.frames.last().unwrap().row_text(0).trim(), "Count 3");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn separated_driver_uses_processed_ack_for_an_ignored_event_without_waiting_for_redraw() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HarnessProbe(journal: journal.clone()));
        let mut frames = Box::pin(ui.mock_terminal_render_loop(config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let acknowledgements = driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![
                        (key(KeyCode::Char('a')), true),
                        (key(KeyCode::Char('a')), true),
                    ],
                    true,
                )
                .await;
            assert!(acknowledgements[1].drawn_frames > acknowledgements[0].drawn_frames);
            let frame_count = driver.frames.len();
            let draws = journal.draw_count();
            let ignored = driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(12)), false)],
                    false,
                )
                .await;
            assert_eq!(ignored[0].sequence, 3);
            assert_eq!(ignored[0].drawn_frames, draws);
            assert_eq!(driver.frames.len(), frame_count);
            assert_eq!(ignored[0].handlers, ["increment", "increment"]);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
