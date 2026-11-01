//! Reuse S2's recording backend and causal driver; quota adds no second emulator.
use futures_util::StreamExt;
use iocraft::{
    Canvas, TerminalEvent,
    prelude::{KeyCode, KeyEventKind, KeyModifiers},
};
use std::time::Duration;
#[path = "../../agent-collaboration-tui/src/test_support/event_driver.rs"]
pub mod event_driver;
#[path = "../../agent-collaboration-tui/src/test_support/frame_grid.rs"]
pub mod frame_grid;
#[path = "../../agent-collaboration-tui/src/test_support/frame_inspection.rs"]
mod frame_inspection;
#[path = "../../agent-collaboration-tui/src/test_support/frame_output.rs"]
pub mod frame_output;
#[path = "../../agent-collaboration-tui/src/test_support/recording_backend.rs"]
pub mod recording_backend;

#[path = "../../agent-collaboration-tui/src/test_support/control_kind_contract.rs"]
pub mod control_kind_contract;
#[path = "../../agent-collaboration-tui/src/test_support/grids.rs"]
pub mod grids;

#[derive(Clone, Default)]
pub(crate) struct PublishedQuotaPopup {
    stack: std::sync::Arc<
        std::sync::Mutex<
            Option<agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle>,
        >,
    >,
}

impl PublishedQuotaPopup {
    pub(crate) fn observer(
        &self,
    ) -> iocraft::Handler<agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle>
    {
        let observed = self.stack.clone();
        iocraft::Handler::from(move |stack| *observed.lock().unwrap() = Some(stack))
    }

    pub(crate) fn frame(&self, canvas: &iocraft::Canvas) -> String {
        let stack = self
            .stack
            .lock()
            .unwrap()
            .expect("real quota stack was observed");
        let snapshot = stack.snapshot();
        assert!(
            matches!(
                snapshot.overlays().last().expect("popup is open").kind,
                agent_collaboration_tui::surfaces::surface_stack::OverlayKind::Dropdown { .. }
            ),
            "popup extraction cannot borrow a dialog or the screen"
        );
        let published = stack
            .hit_regions()
            .published()
            .expect("current published draw");
        let region = published
            .surfaces
            .iter()
            .find(|region| region.surface == snapshot.top_id())
            .expect("published popup surface");
        published_popup_frame(canvas, &region.cells)
    }
}

pub(crate) fn published_popup_frame(canvas: &iocraft::Canvas, cells: &[(u16, u16)]) -> String {
    let left = usize::from(
        cells
            .iter()
            .map(|cell| cell.0)
            .min()
            .expect("published popup has cells"),
    );
    let right = usize::from(cells.iter().map(|cell| cell.0).max().unwrap());
    let top = usize::from(cells.iter().map(|cell| cell.1).min().unwrap());
    let bottom = usize::from(cells.iter().map(|cell| cell.1).max().unwrap());
    assert!(
        right > left && bottom > top,
        "published popup has a framed rectangle"
    );
    let grid = frame_grid::FrameGrid::from_canvas(canvas).expect("actual Canvas serializes");
    let text = |column, row| {
        grid.cell(column, row)
            .expect("published popup cell is in this Canvas")
            .text
            .as_str()
    };
    assert_eq!(text(left, top), "╭", "published popup top border");
    assert_eq!(text(right, top), "╮", "published popup top border");
    assert!(
        (left + 1..right).all(|column| text(column, top) == "─"),
        "published popup top border"
    );
    assert_eq!(text(left, bottom), "╰", "published popup bottom border");
    assert_eq!(text(right, bottom), "╯", "published popup bottom border");
    assert!(
        (left + 1..right).all(|column| text(column, bottom) == "─"),
        "published popup bottom border"
    );
    (top..=bottom)
        .map(|row| {
            grid.cells[row][left..=right]
                .iter()
                .map(|cell| {
                    if cell.continuation {
                        ""
                    } else if cell.text.is_empty() {
                        " "
                    } else {
                        cell.text.as_str()
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn published_popup_frame_excludes_parent_text_and_same_row_corners() {
    use iocraft::prelude::*;
    let canvas = element!(Text(content:"Allow outside\n╭─────╭────────────╮╮\n│     │Filter:     ││\n│     │● Disallow  ││\n╰─────│○ Allow     │╯\n      ╰────────────╯")).render(None);
    let popup = published_popup_frame(&canvas, &popup_test_region());
    assert!(
        !popup.contains("outside"),
        "a parent value cannot impersonate an option"
    );
    let rows = popup.lines().collect::<Vec<_>>();
    assert_eq!(rows.first().copied(), Some("╭────────────╮"));
    assert_eq!(rows.last().copied(), Some("╰────────────╯"));
    assert!(popup.find("Disallow").unwrap() < popup.find("Allow").unwrap());
}

#[test]
#[should_panic(expected = "published popup bottom border")]
fn published_popup_frame_rejects_missing_bottom_even_with_parent_border() {
    use iocraft::prelude::*;
    let canvas = element!(Text(content:"Allow outside\n╭─────╭────────────╮╮\n│     │Filter:     ││\n│     │● Disallow  ││\n╰─────│○ Allow     │╯\n                   ")).render(None);
    published_popup_frame(&canvas, &popup_test_region());
}

fn popup_test_region() -> Vec<(u16, u16)> {
    (1..=5)
        .flat_map(|row| (6..=19).map(move |column| (column, row)))
        .collect()
}

pub fn shift_tab_event() -> iocraft::TerminalEvent {
    use iocraft::prelude::*;
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Tab);
    key.modifiers = KeyModifiers::SHIFT;
    TerminalEvent::Key(key)
}

pub fn require_color_environment() {
    assert!(
        std::env::var_os("NO_COLOR").is_none(),
        "quota frame checks require NO_COLOR unset"
    );
}

pub(crate) fn dispatched_frame_sequences(bytes: &[u8]) -> Vec<u64> {
    let text = std::str::from_utf8(bytes).unwrap();
    let (_, mut suffix) = text
        .rsplit_once("\x1b[?2026l")
        .expect("shared native backend ends every synchronized frame");
    let mut sequences = Vec::new();
    while !suffix.is_empty() {
        let next = suffix
            .strip_prefix("\x1b]7770;dispatch=")
            .expect("only dispatch markers follow the frame flush");
        let (sequence, rest) = next
            .split_once("\x1b\\")
            .expect("complete dispatch marker terminator");
        sequences.push(sequence.parse().expect("dispatch sequence is a u64"));
        suffix = rest;
    }
    sequences
}

pub(crate) struct QuotaInputFrameDriver {
    sender: tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
    receipt_path: std::path::PathBuf,
    sequence: u64,
}
impl QuotaInputFrameDriver {
    pub(crate) fn new(
        sender: tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
        receipt_path: std::path::PathBuf,
    ) -> Self {
        Self {
            sender,
            receipt_path,
            sequence: 0,
        }
    }

    pub(crate) async fn step<TFrames>(
        &mut self,
        mut frames: std::pin::Pin<&mut TFrames>,
        event: TerminalEvent,
        mut matches: impl FnMut(&Canvas) -> bool,
    ) -> Canvas
    where
        TFrames: futures_util::Stream<Item = frame_output::DispatchFrame> + ?Sized,
    {
        self.sequence = self.sequence.checked_add(1).unwrap();
        let sequence = self.sequence;
        let marker_sequence = sequence;
        let expected_event = quota_event_readback(&event);
        self.sender.send(event).unwrap();
        let mut receipt_seen = false;
        let mut marker_seen = false;
        let mut last_frame = String::new();
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let (canvas, markers) = current_quota_batch(frames.as_mut()).await;
                let receipts = std::fs::read_to_string(&self.receipt_path)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                    .collect::<Vec<_>>();
                if let Some(receipt) = receipts.iter().find(|value| value["sequence"] == sequence) {
                    assert_eq!(receipts.len(), usize::try_from(sequence).unwrap());
                    assert_eq!(
                        receipt["event"], expected_event,
                        "actual event for this step"
                    );
                    receipt_seen = true;
                }
                marker_seen |= markers.contains(&marker_sequence);
                if marker_seen {
                    assert!(
                        receipt_seen,
                        "flush marker must have its actual event receipt"
                    );
                }
                if let Some(canvas) = canvas {
                    last_frame = canvas.to_string();
                    if receipt_seen && marker_seen && matches(&canvas) {
                        return canvas;
                    }
                }
            }
        })
        .await;
        result.unwrap_or_else(|error| {
            panic!(
                "quota causal step {sequence}: {error}; receipt={receipt_seen}, marker={marker_seen}, expected marker={marker_sequence}; last frame:\n{last_frame}"
            )
        })
    }
}

fn quota_event_readback(event: &TerminalEvent) -> serde_json::Value {
    let TerminalEvent::Key(key) = event else {
        panic!("quota journey uses only its declared key inputs");
    };
    assert_eq!(key.kind, KeyEventKind::Press);
    let (code, modifiers) = match key.code {
        KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => (
            serde_json::json!({"kind":"shortcut","key":"r"}),
            serde_json::json!(["control"]),
        ),
        KeyCode::Tab if key.modifiers == KeyModifiers::SHIFT => (
            serde_json::json!({"kind":"fixed","name":KeyCode::Tab.to_string()}),
            serde_json::json!(["shift"]),
        ),
        code if key.modifiers.is_empty() => (
            serde_json::json!({"kind":"fixed","name":code.to_string()}),
            serde_json::json!([]),
        ),
        _ => panic!("quota journey received an undeclared key chord"),
    };
    serde_json::json!({"kind":"key","code":code,"modifiers":modifiers,"phase":"press"})
}

pub(crate) async fn current_quota_canvas<TFrames>(mut frames: std::pin::Pin<&mut TFrames>) -> Canvas
where
    TFrames: futures_util::Stream<Item = frame_output::DispatchFrame> + ?Sized,
{
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (canvas, markers) = current_quota_batch(frames.as_mut()).await;
            assert!(markers.is_empty(), "initial paint precedes all input");
            if let Some(canvas) = canvas {
                return canvas;
            }
        }
    })
    .await
    .expect("initial quota Canvas must paint")
}

async fn current_quota_batch<TFrames>(
    mut frames: std::pin::Pin<&mut TFrames>,
) -> (Option<Canvas>, Vec<u64>)
where
    TFrames: futures_util::Stream<Item = frame_output::DispatchFrame> + ?Sized,
{
    use futures_util::FutureExt;
    let mut frame = frames.next().await.expect("quota render stream is live");
    let mut canvas = None;
    let mut markers = Vec::new();
    for _ in 0..64 {
        let sequences = dispatched_frame_sequences(&frame.bytes);
        assert!(
            sequences.is_empty() || frame.canvas.is_some(),
            "quota marker follows this frame's actual Canvas, never a cached frame"
        );
        markers.extend(sequences);
        if let Some(painted) = frame.canvas {
            canvas = Some(painted);
        }
        match frames.next().now_or_never() {
            Some(Some(painted)) => frame = painted,
            _ => return (canvas, markers),
        }
    }
    panic!("quota ready-frame drain exceeded64")
}
