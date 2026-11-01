//! Cross-process public-API integration: real native frame bytes, marker transport, child exit and terminal outcomes.
#[cfg(test)]
mod tests {
    use crate::test_support::{
        dispatch_frame_scene::DispatchScene,
        frame_output::{SharedFrameOutput, dispatch_render_loop},
    };
    use futures_util::{FutureExt, StreamExt};
    use iocraft::prelude::*;
    use tokio::sync::mpsc;
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }

    #[tokio::test]
    async fn unconfigured_input_owner_emits_no_marker_and_no_forced_frame_for_ignored_input() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let config = MockTerminalConfig::with_events(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
        ));
        let bytes = SharedFrameOutput::default();
        let (acknowledged, mut acknowledgements) = mpsc::unbounded_channel();
        let mut ui = element!(DispatchScene(on_routed:move|()|{acknowledged.send(()).unwrap();}));
        let mut frames = Box::pin(dispatch_render_loop(
            &mut ui,
            config,
            bytes.clone(),
            Box::new(bytes.clone()),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let initial = frames.next().await.unwrap();
            assert!(initial.canvas.is_some());
            assert!(initial.bytes.starts_with(b"\x1b[?2026h"));
            while let Some(Some(_)) = frames.next().now_or_never() {}
            let baseline = bytes.bytes();
            sender.send(key(KeyCode::Home)).unwrap();
            loop {
                tokio::select! {
                    event=acknowledgements.recv()=>{event.unwrap();break;},
                    frame=frames.next()=>{frame.unwrap();}
                }
            }
            while let Some(Some(_)) = frames.next().now_or_never() {}
            assert_eq!(
                bytes.bytes(),
                baseline,
                "unset receipts preserve the exact bytes after a real ignored Home"
            );
            assert!(!baseline.windows(4).any(|bytes| bytes == b"7770"));
        })
        .await
        .unwrap();
    }
    #[cfg(all(feature = "test-support", debug_assertions))]
    #[tokio::test]
    async fn dispatch_marker_follows_the_changed_frame_bytes() {
        use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
        let directory = tempfile::tempdir().unwrap();
        let sink = InputDispatchReceiptSink::create(directory.path().join("input-dispatch.jsonl"))
            .unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let config = MockTerminalConfig::with_events(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
        ));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let mut ui = element!(DispatchScene(sink:Some(sink.clone())));
        let mut frames = Box::pin(dispatch_render_loop(&mut ui, config, bytes, writer));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            frames.next().await.unwrap();
            while let Some(Some(_)) = frames.next().now_or_never() {}
            sender.send(key(KeyCode::F(8))).unwrap();
            let frame = frames.next().await.unwrap();
            assert!(frame.canvas.unwrap().to_string().contains("Value 1"));
            assert!(
                frame
                    .bytes
                    .ends_with(b"\x1b[?2026l\x1b]7770;dispatch=1\x1b\\"),
                "the post-dispatch marker must follow the flushed changed frame: {:?}",
                String::from_utf8_lossy(&frame.bytes)
            );
        })
        .await
        .unwrap();
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    #[tokio::test]
    async fn ignored_home_forces_an_identical_full_frame_and_a_marker_without_waiting_on_a_redraw()
    {
        use crate::test_support::frame_grid::FrameGrid;
        use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let config = MockTerminalConfig::with_events(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
        ));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let (acknowledged, mut acknowledgements) = mpsc::unbounded_channel();
        let mut ui = element!(DispatchScene(sink:Some(sink.clone()),on_routed:move |()|{acknowledged.send(()).unwrap();}));
        let mut frames = Box::pin(dispatch_render_loop(&mut ui, config, bytes, writer));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let before = frames.next().await.unwrap().canvas.unwrap();
            while let Some(Some(_)) = frames.next().now_or_never() {}
            sender.send(key(KeyCode::Home)).unwrap();
            let mut observed = Vec::new();
            loop {
                tokio::select! {
                    event = acknowledgements.recv() => { event.unwrap(); break; },
                    frame = frames.next() => observed.push(frame.unwrap()),
                }
            }
            while let Some(Some(frame)) = frames.next().now_or_never() {
                observed.push(frame);
            }
            assert_eq!(
                std::fs::read_to_string(&path).unwrap().lines().count(),
                1,
                "the actual ignored Home was consumed before checking its frame"
            );
            assert_eq!(
                observed.len(),
                1,
                "even a no-op receipt must force its post-dispatch frame"
            );
            let frame = observed.pop().unwrap();
            assert!(
                frame.canvas.as_ref() == Some(&before),
                "the forced frame serializes the same cells"
            );
            let suffix = b"\x1b[?2026l\x1b]7770;dispatch=1\x1b\\";
            assert!(
                frame.bytes.ends_with(suffix),
                "no-op marker follows its frame flush"
            );
            let content = &frame.bytes[b"\x1b[?2026h".len()..frame.bytes.len() - suffix.len()];
            let mut actual = FrameGrid::new(before.width(), before.height());
            actual.apply(std::str::from_utf8(content).unwrap()).unwrap();
            assert_eq!(
                actual,
                FrameGrid::from_canvas(&before).unwrap(),
                "the OSC adds or changes no grid cell"
            );
        })
        .await
        .unwrap();
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    #[tokio::test]
    async fn burst_receipts_share_the_post_burst_frame_and_keep_sequence_order() {
        use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input-dispatch.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let config = MockTerminalConfig::with_events(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
        ));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let mut ui = element!(DispatchScene(sink:Some(sink.clone())));
        let mut frames = Box::pin(dispatch_render_loop(&mut ui, config, bytes, writer));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            frames.next().await.unwrap();
            while let Some(Some(_)) = frames.next().now_or_never() {}
            for event in [key(KeyCode::Home), key(KeyCode::F(8)), key(KeyCode::Home)] { sender.send(event).unwrap(); }
            let frame = frames.next().await.unwrap();
            assert!(frame.canvas.unwrap().to_string().contains("Value 1"));
            assert!(frame.bytes.ends_with(b"\x1b[?2026l\x1b]7770;dispatch=1\x1b\\\x1b]7770;dispatch=2\x1b\\\x1b]7770;dispatch=3\x1b\\"));
            assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 3);
        }).await.unwrap();
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    #[tokio::test]
    async fn frame_flush_failure_is_retained_for_the_fixture_when_the_pin_discards_end_frame_errors()
     {
        use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
        use std::{
            io::{self, Write},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };
        struct FailingFrameFlush {
            bytes: SharedFrameOutput,
            armed: Arc<AtomicBool>,
            attempted: Arc<AtomicBool>,
            end_frame: bool,
        }
        impl Write for FailingFrameFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.end_frame |= bytes == b"\x1b[?2026l";
                self.bytes.write(bytes)
            }
            fn flush(&mut self) -> io::Result<()> {
                if std::mem::take(&mut self.end_frame) && self.armed.load(Ordering::SeqCst) {
                    self.attempted.store(true, Ordering::SeqCst);
                    return Err(io::Error::other("native end-frame flush rejected"));
                }
                self.bytes.flush()
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let sink = InputDispatchReceiptSink::create(directory.path().join("input-dispatch.jsonl"))
            .unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let config = MockTerminalConfig::with_events(futures_util::stream::unfold(
            receiver,
            |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
        ));
        let bytes = SharedFrameOutput::default();
        let armed = Arc::new(AtomicBool::new(false));
        let attempted = Arc::new(AtomicBool::new(false));
        let writer = Box::new(
            sink.frame_writer(FailingFrameFlush {
                bytes: bytes.clone(),
                armed: armed.clone(),
                attempted: attempted.clone(),
                end_frame: false,
            })
            .unwrap(),
        );
        let (acknowledged, mut acknowledgements) = mpsc::unbounded_channel();
        let mut ui = element!(DispatchScene(sink:Some(sink.clone()),on_routed:move|()|{acknowledged.send(()).unwrap();}));
        let mut frames = Box::pin(dispatch_render_loop(&mut ui, config, bytes, writer));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            frames.next().await.unwrap();
            while let Some(Some(_)) = frames.next().now_or_never() {}
            armed.store(true, Ordering::SeqCst);
            sender.send(key(KeyCode::F(8))).unwrap();
            loop {
                tokio::select! {
                    event=acknowledgements.recv()=>{event.unwrap();break;},
                    frame=frames.next()=>{frame.unwrap();}
                }
            }
            while let Some(Some(_)) = frames.next().now_or_never() {}
            assert!(
                attempted.load(Ordering::SeqCst),
                "the real native loop reached the failing end-frame flush"
            );
            let error = sink.flush_and_check().unwrap_err();
            assert_eq!(error.to_string(), "native end-frame flush rejected");
            assert_eq!(
                sink.flush_and_check().unwrap_err().to_string(),
                error.to_string(),
                "checking never clears a frame-write failure"
            );
        })
        .await
        .unwrap();
    }

    async fn run_terminal_child(mode: &str) -> (Vec<u8>, Option<Vec<u8>>) {
        let directory = tempfile::tempdir().unwrap();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "dispatch_frames::tests::dispatch_frame_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("TUI_FRAME_CHILD_DIRECTORY", directory.path())
                .env("TUI_FRAME_CHILD_MODE", mode)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "child status {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.windows(4).any(|bytes| bytes == b"7770"),
            "a marker escaped the chosen render stream"
        );
        assert_eq!(
            std::fs::read(directory.path().join("outcome.json")).unwrap(),
            b"{\"kind\":\"quit\"}\n"
        );
        let receipts = std::fs::read(directory.path().join("input-dispatch.jsonl")).ok();
        (
            std::fs::read(directory.path().join("frame-bytes.ansi")).unwrap(),
            receipts,
        )
    }

    #[tokio::test]
    async fn unconfigured_child_exits_with_the_outcome_and_no_marker_or_receipt() {
        let (bytes, receipts) = run_terminal_child("unconfigured").await;
        assert!(!bytes.windows(4).any(|bytes| bytes == b"7770"));
        assert!(receipts.is_none());
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    #[tokio::test]
    async fn terminal_driver_finishes_on_exit_and_outcome_with_or_without_a_final_marker() {
        for (mode, marker) in [("terminal-marker", true), ("terminal-no-marker", false)] {
            let (bytes, receipts) = run_terminal_child(mode).await;
            assert_eq!(bytes.windows(4).any(|bytes| bytes == b"7770"), marker);
            let receipt: serde_json::Value = serde_json::from_slice(&receipts.unwrap()).unwrap();
            assert_eq!(receipt["sequence"], 1);
            assert_eq!(receipt["action"], "quit");
        }
    }

    #[tokio::test]
    #[ignore = "controlled subprocess fixture; invoked by the terminal driver tests"]
    async fn dispatch_frame_child() {
        let directory =
            std::path::PathBuf::from(std::env::var_os("TUI_FRAME_CHILD_DIRECTORY").unwrap());
        let mode = std::env::var("TUI_FRAME_CHILD_MODE").unwrap();
        assert!(matches!(
            mode.as_str(),
            "unconfigured"
                | "terminal-marker"
                | "terminal-no-marker"
                | "native-tty-marker"
                | "native-tty-unconfigured"
        ));
        let bytes = SharedFrameOutput::default();
        let writer: Box<dyn std::io::Write + Send> = Box::new(bytes.clone());
        let ui = element!(DispatchScene);
        #[cfg(all(feature = "test-support", debug_assertions))]
        let sink =
            (!matches!(mode.as_str(), "unconfigured" | "native-tty-unconfigured")).then(|| {
                agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink::create(
                    directory.join("input-dispatch.jsonl"),
                )
                .unwrap()
            });
        #[cfg(all(feature = "test-support", debug_assertions))]
        let (ui, writer) = {
            let mut ui = ui;
            ui.props.sink = sink.clone();
            let writer: Box<dyn std::io::Write + Send> = if mode == "terminal-marker" {
                Box::new(sink.as_ref().unwrap().frame_writer(writer).unwrap())
            } else {
                writer
            };
            (ui, writer)
        };
        let mut ui = ui;
        let mut quit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
        quit.modifiers = KeyModifiers::CONTROL;
        if mode.starts_with("native-tty-") {
            let render = ui.render_loop().fullscreen().ignore_ctrl_c();
            #[cfg(all(feature = "test-support", debug_assertions))]
            let render = if let Some(sink) = &sink {
                render.stdout(sink.frame_writer(std::io::stdout()).unwrap())
            } else {
                render
            };
            render.await.unwrap();
        } else {
            let config =
                MockTerminalConfig::with_events(futures_util::stream::iter([TerminalEvent::Key(
                    quit,
                )]));
            let mut frames = Box::pin(dispatch_render_loop(&mut ui, config, bytes.clone(), writer));
            while frames.next().await.is_some() {}
        }
        #[cfg(all(feature = "test-support", debug_assertions))]
        if let Some(sink) = sink {
            sink.flush_and_check().unwrap();
        }
        std::fs::write(directory.join("frame-bytes.ansi"), bytes.bytes()).unwrap();
        std::fs::write(directory.join("outcome.json"), b"{\"kind\":\"quit\"}\n").unwrap();
    }
}
