use crate::support::{
    event_driver::{EventDriver, InputMode},
    list_scene::{ListHandle, ListScene, key},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{controls::ListNavigationPolicy, theme::UiGlyph};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn content_scroll_pages_and_bounds_keep_selection_while_arrows_select() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = ListHandle::default();
        let activated = Arc::new(Mutex::new(Vec::new()));
        let selections = Arc::new(Mutex::new(Vec::new()));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ListScene(journal,handle:handle.clone(),activated:activated.clone(),selections:selections.clone(),navigation_policy:ListNavigationPolicy::ContentScroll));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            assert_eq!(fixture.control.scroll().position().maximum(), 3);
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    [
                        KeyCode::PageDown,
                        KeyCode::End,
                        KeyCode::PageUp,
                        KeyCode::Home,
                    ]
                    .into_iter()
                    .map(|code| (key(code), true))
                    .collect(),
                    false,
                    |ack| {
                        assert_eq!(
                            ack.snapshot.0[0].1, b"alpha|list",
                            "{mode:?}: content scroll must retain Alpha"
                        );
                        let offset = String::from_utf8(ack.snapshot.0[1].1.clone())
                            .unwrap()
                            .parse::<u16>()
                            .unwrap();
                        assert_eq!(
                            offset,
                            [2, 3, 1, 0][ack.sequence - 1],
                            "{mode:?}: actual shared scroll offset"
                        );
                    },
                )
                .await;
            assert!(selections.lock().unwrap().is_empty());
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    [KeyCode::Down, KeyCode::Up, KeyCode::Enter]
                        .into_iter()
                        .map(|code| (key(code), true))
                        .collect(),
                    true,
                    |ack| {
                        assert_eq!(
                            ack.snapshot.0[0].1,
                            if ack.sequence == 5 {
                                b"bravo|list".as_slice()
                            } else {
                                b"alpha|list".as_slice()
                            }
                        )
                    },
                )
                .await;
            assert_eq!(*activated.lock().unwrap(), vec!["alpha"]);
            assert_eq!(
                *selections.lock().unwrap(),
                vec![Some("bravo".into()), Some("alpha".into())]
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn content_scroll_wheel_uses_the_scroll_handle_without_selecting_the_row_under_it() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = ListHandle::default();
        let selections = Arc::new(Mutex::new(Vec::new()));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(ListScene(journal,handle:handle.clone(),selections:selections.clone(),navigation_policy:ListNavigationPolicy::ContentScroll));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            let map = fixture.stack.hit_regions().published().unwrap();
            let point = *map
                .targets
                .iter()
                .find(|region| region.target == fixture.control.scroll_target())
                .unwrap()
                .cells
                .first()
                .unwrap();
            driver
                .drive_checked(
                    &mut frames,
                    mode,
                    vec![(
                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::ScrollDown,
                            point.0,
                            point.1,
                        )),
                        true,
                    )],
                    true,
                    |ack| {
                        assert_eq!(
                            ack.snapshot.0[0].1, b"alpha|list",
                            "wheel must preserve the selected identity"
                        );
                        assert_eq!(fixture.control.scroll().position().offset, 1);
                    },
                )
                .await;
            assert!(selections.lock().unwrap().is_empty());
            assert_eq!(fixture.control.selected().unwrap().as_str(), "alpha");
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn list_row_marker_offset_places_the_focus_marker_below_the_group_heading() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(ListScene(journal,marker_offset:2u16,viewport_rows:Some(4u16)));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let frame = driver.frames.last().unwrap();
        assert!(
            frame.row_text(1).starts_with("  alpha"),
            "heading row has no focus marker: {}",
            frame.row_text(1)
        );
        assert!(!frame.row_text(1).starts_with(UiGlyph::Focus.text()));
        assert!(!frame.row_text(2).starts_with(UiGlyph::Focus.text()));
        assert!(
            frame.row_text(3).starts_with(UiGlyph::Focus.text()),
            "focus marker is at column 0 of the third group row: {}",
            frame.row_text(3)
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
