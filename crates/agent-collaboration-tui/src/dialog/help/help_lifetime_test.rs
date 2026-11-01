use crate::{
    test_support::help_scene::{key, text},
    test_support::{
        dialog_scene::DialogScene,
        event_driver::{EventDriver, InputMode},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[tokio::test]
async fn dialog_help_swaps_one_body_and_restores_query_focus_and_draft_byte_for_byte() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let data = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(DialogScene(journal,form:true,width:76u16,height:30u16,data:data.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let prepared = driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::Enter), true),
                        (TerminalEvent::BracketedPaste("remote".into()), true),
                        (key(KeyCode::Esc), true),
                        (key(KeyCode::Down), true),
                        (key(KeyCode::Left), true),
                        (TerminalEvent::BracketedPaste("X".into()), true),
                    ],
                    true,
                    |ack| {
                        if ack.sequence == 2 {
                            assert_eq!(
                                ack.snapshot.0.len(),
                                3,
                                "the real dropdown must receive the query"
                            );
                            assert_eq!(ack.snapshot.0[2].1, b"remote|beta");
                        }
                    },
                )
                .await;
            let before = prepared.last().unwrap().snapshot.clone();
            let canvas_before = driver.frames.last().unwrap().clone();
            let data = data.lock().unwrap().unwrap();
            let query_before = data.client.read().clone();
            let draft_before = data.directory.read().clone();
            let opened = driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![(key(KeyCode::F(1)), true)],
                    true,
                    |ack| {
                        assert_eq!(ack.snapshot.0.len(), 2, "Help must not add a second dialog");
                        assert_eq!(ack.snapshot.0[1].0, before.0[1].0);
                    },
                )
                .await;
            assert_eq!(opened.len(), 1);
            let help = text(&driver);
            assert!(help.contains("Close Help"));
            assert!(
                help.lines()
                    .any(|line| line.contains("Ctrl+Enter") && line.contains("Start"))
            );
            assert!(
                help.lines()
                    .any(|line| line.contains("Esc") && line.contains("Close"))
            );
            assert!(!help.contains("Connection"));
            let closed = driver
                .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                .await;
            assert_eq!(closed.last().unwrap().snapshot, before);
            assert_eq!(*data.client.read(), query_before);
            assert_eq!(*data.directory.read(), draft_before);
            assert_eq!(driver.frames.last().unwrap(), &canvas_before);
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}
