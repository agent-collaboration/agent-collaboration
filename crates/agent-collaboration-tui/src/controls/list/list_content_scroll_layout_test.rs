//! Frozen ungrouped ContentScroll bytes captured before grouped reveal changes.
use crate::controls::list::{ListNavigationPolicy, ListRowKey};
use crate::test_support::{
    event_driver::{EventDriver, InputMode},
    grids::STANDARD_GRIDS,
    list_scene::{ListHandle, ListScene, key},
    recording_backend::recording_terminal_render_loop,
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn ungrouped_content_scroll_keeps_native_bytes_at_standard_grids() {
    for (width, height) in STANDARD_GRIDS {
        for marker_offset in [0u16, 2] {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let captured = Arc::new(Mutex::new(None));
            let output = captured.clone();
            let rows = (1..=40)
                .map(|number| ListRowKey::new(format!("Session {number:02}")).unwrap())
                .collect();
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),marker_offset,initial_rows:Some(crate::controls::list::ListRows::Ungrouped(rows)),navigation_policy:ListNavigationPolicy::ContentScroll));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
                move |canvas| {
                    let mut bytes = Vec::new();
                    canvas.write_ansi(&mut bytes).unwrap();
                    *output.lock().unwrap() = Some(bytes);
                },
            ));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                for (position, selected) in [("top", "Session 01"), ("end", "Session 01")] {
                    if position == "end" {
                        driver
                            .drive(
                                &mut frames,
                                InputMode::FrameSeparated,
                                vec![(key(KeyCode::End), true)],
                                true,
                            )
                            .await;
                    }
                    assert_eq!(handle.get().control.selected().unwrap().as_str(), selected);
                    let frame = driver.frames.last().unwrap();
                    assert!(
                        (0..frame.height).any(|row| frame.row_text(row).contains(
                            if position == "end" {
                                "Session 40"
                            } else {
                                selected
                            }
                        )),
                        "the expected content bound is painted"
                    );
                    let bytes = captured.lock().unwrap().clone().unwrap();
                    assert_eq!(bytes, baseline(width, height, marker_offset, position), "ungrouped ContentScroll native bytes at {width}x{height}, marker{marker_offset}, {position}");
                }
            })
            .await
            .unwrap();
        }
    }
}

fn baseline(width: u16, height: u16, marker_offset: u16, position: &str) -> &'static [u8] {
    match (width, height, marker_offset, position) {
        (120, 36, 0, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/120x36-marker0-top.ansi")
        }
        (120, 36, 0, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/120x36-marker0-end.ansi")
        }
        (120, 36, 2, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/120x36-marker2-top.ansi")
        }
        (120, 36, 2, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/120x36-marker2-end.ansi")
        }
        (76, 30, 0, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/76x30-marker0-top.ansi")
        }
        (76, 30, 0, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/76x30-marker0-end.ansi")
        }
        (76, 30, 2, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/76x30-marker2-top.ansi")
        }
        (76, 30, 2, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/76x30-marker2-end.ansi")
        }
        (48, 12, 0, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/48x12-marker0-top.ansi")
        }
        (48, 12, 0, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/48x12-marker0-end.ansi")
        }
        (48, 12, 2, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/48x12-marker2-top.ansi")
        }
        (48, 12, 2, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/48x12-marker2-end.ansi")
        }
        (32, 24, 0, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/32x24-marker0-top.ansi")
        }
        (32, 24, 0, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/32x24-marker0-end.ansi")
        }
        (32, 24, 2, "top") => {
            include_bytes!("fixtures/content-scroll-ungrouped/32x24-marker2-top.ansi")
        }
        (32, 24, 2, "end") => {
            include_bytes!("fixtures/content-scroll-ungrouped/32x24-marker2-end.ansi")
        }
        _ => panic!("baseline outside the canonical grids"),
    }
}
