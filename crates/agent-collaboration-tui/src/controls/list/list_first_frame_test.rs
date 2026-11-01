//! Inspect every native frame rather than the driver's last settled canvas.
#[cfg(test)]
mod initial_frame {
    use crate::test_support::{
        event_driver::EventDriver, frame_grid::FrameGrid, list_scene::ListScene,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{controls::list::ListRowKey, foundation::theme::UiGlyph};
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    async fn first_list(width: u16, height: u16, count: usize, overflow: bool) {
        let (mut driver, config, journal) = EventDriver::new();
        let first = Arc::new(Mutex::new(None));
        let captured = first.clone();
        let rows = (1..=count)
            .map(|number| ListRowKey::new(format!("Session {number:02}")).unwrap())
            .collect();
        let mut ui = element!(ListScene(journal,width:Some(width),height:Some(height),viewport_rows:Some(height-4),initial_rows:Some(rows)));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
            move |canvas| {
                captured
                    .lock()
                    .unwrap()
                    .get_or_insert_with(|| FrameGrid::from_canvas(canvas).unwrap());
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let first = first.lock().unwrap();
            let first = first.as_ref().unwrap();
            let text = (0..first.height)
                .map(|row| first.row_text(row))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                text.contains(UiGlyph::ScrollThumb.text()),
                overflow,
                "first unconfigured list frame must show its actual overflow\n{text}"
            );
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn fresh_list_first_frame_exposes_overflow_without_receipts() {
        first_list(48, 12, 40, true).await;
        first_list(32, 24, 40, true).await;
    }
    #[tokio::test]
    async fn fresh_list_first_frame_has_no_track_when_content_fits() {
        first_list(48, 12, 5, false).await;
    }
}
#[cfg(all(test, feature = "test-support", debug_assertions))]
mod tests {
    use crate::test_support::{
        event_driver::{EventDriver, InputMode},
        frame_grid::FrameGrid,
        frame_output::{SharedFrameOutput, dispatch_render_loop},
        list_scene::{ListHandle, ListScene},
    };
    use agent_collaboration_tui::{
        controls::list::ListRowKey, foundation::theme::UiGlyph,
        input::input_dispatch_receipt::InputDispatchReceiptSink,
    };
    use futures_util::StreamExt;
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    async fn jump_last(width: u16, height: u16) {
        let directory = tempfile::tempdir().unwrap();
        let sink = InputDispatchReceiptSink::create(directory.path().join("input-dispatch.jsonl"))
            .unwrap();
        let (mut driver, config, journal) = EventDriver::new();
        let handle = ListHandle::default();
        let rows = (1..=40)
            .map(|number| ListRowKey::new(format!("Session {number:02}")).unwrap())
            .collect();
        let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),initial_rows:Some(rows),viewport_rows:Some(height-4),marker_offset:2u16,sink:Some(sink.clone())));
        let output = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(output.clone()).unwrap());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured = observed.clone();
        let mut frames = Box::pin(
            dispatch_render_loop(&mut ui, config, output, writer)
                .inspect(move |frame| {
                    captured.lock().unwrap().push((
                        frame
                            .canvas
                            .as_ref()
                            .map(|canvas| FrameGrid::from_canvas(canvas).unwrap()),
                        frame.bytes.clone(),
                    ));
                })
                .filter_map(|frame| async move { frame.canvas }),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            // Drain setup only. The observer retains the FIRST canvas of the later input.
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            assert_eq!(fixture.control.selected().unwrap().as_str(), "Session 01");
            assert_eq!(fixture.control.scroll().position().offset, 0);
            let before = observed.lock().unwrap().len();
            let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Down);
            event.modifiers = KeyModifiers::ALT | KeyModifiers::SHIFT;
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(TerminalEvent::Key(event), true)],
                    true,
                )
                .await;
            assert_eq!(fixture.control.selected().unwrap().as_str(), "Session 40");
            let observed = observed.lock().unwrap();
            let (first, bytes) = &observed[before];
            let first = first
                .as_ref()
                .expect("receipt forces the first post-dispatch canvas");
            let text = (0..first.height)
                .map(|row| first.row_text(row))
                .collect::<Vec<_>>()
                .join("\n");
            eprintln!("FIRST POST-DISPATCH {width}x{height}:\n{text}");
            assert!(bytes.ends_with(b"\x1b[?2026l\x1b]7770;dispatch=1\x1b\\"));
            assert!(
                text.contains("Session 40"),
                "{width}x{height}: first frame must reveal last selection\n{text}"
            );
            assert!(
                text.contains(UiGlyph::Focus.text()),
                "{width}x{height}: selected row's focus marker must already be visible\n{text}"
            );
            assert!(
                !text.contains("Session 01"),
                "first frame must leave initial window"
            );
            sink.flush_and_check().unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn jump_last_paints_selection_in_first_native_frame_at_48x12() {
        jump_last(48, 12).await;
    }
    #[tokio::test]
    async fn jump_last_paints_selection_in_first_native_frame_at_32x24() {
        jump_last(32, 24).await;
    }
}
