use crate::test_support::{
    dialog_scene::DialogScene, event_driver::EventDriver, frame_grid::FrameGrid,
    recording_backend::recording_terminal_render_loop,
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

async fn record_dialogs_without_slots() -> (Vec<u8>, String) {
    let mut serialized = Vec::new();
    let mut grids = String::new();
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        for form in [false, true] {
            let (mut driver, config, journal) = EventDriver::new();
            let last_canvas = Arc::new(Mutex::new(None));
            let captured = last_canvas.clone();
            let mut ui = element!(DialogScene(journal, width, height, form));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
                move |canvas| {
                    *captured.lock().unwrap() = Some(canvas.clone());
                },
            ));
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                driver.initial(&mut frames),
            )
            .await
            .unwrap();
            let canvas = last_canvas.lock().unwrap().take().unwrap();
            let grid = FrameGrid::from_canvas(&canvas).unwrap();
            let label = format!("{width}x{height}, form={form}\n");
            serialized.extend_from_slice(label.as_bytes());
            canvas.write_ansi(&mut serialized).unwrap();
            grids.push_str(&label);
            for row in 0..grid.height {
                grids.push_str(&grid.row_text(row));
                grids.push('\n');
            }
        }
    }
    (serialized, grids)
}

#[tokio::test]
async fn empty_fixed_slots_preserve_original_dialog_bytes_at_all_standard_grids() {
    let (actual, _) = record_dialogs_without_slots().await;
    let expected = include_bytes!("test_fixtures/dialog-no-fixed-slots.ansi");
    let first_difference = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected);
    assert!(
        actual == expected,
        "no-slot native ANSI changed at {first_difference:?}; actual={} bytes, baseline={} bytes",
        actual.len(),
        expected.len()
    );
}

/// Explicit export keeps golden regeneration out of normal test execution.
#[tokio::test]
#[ignore = "exports a reviewable baseline into an explicit new owned file"]
async fn export_no_fixed_slots_baseline() {
    let path = std::path::PathBuf::from(
        std::env::var_os("TUI_DIALOG_BASELINE_EXPORT").expect("absolute export file"),
    );
    assert!(path.is_absolute());
    let (bytes, grids) = record_dialogs_without_slots().await;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
    eprintln!(
        "NO-SLOT BASELINE, {} bytes, exported to {}:\n{grids}",
        bytes.len(),
        path.display()
    );
}
