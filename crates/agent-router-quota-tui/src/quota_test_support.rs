//! Reuse S2's recording backend and causal driver; quota adds no second emulator.
#[path = "../../agent-collaboration-tui/tests/support/dispatch_frame_scene.rs"]
pub mod dispatch_frame_scene;
#[path = "../../agent-collaboration-tui/tests/support/event_driver.rs"]
pub mod event_driver;
#[path = "../../agent-collaboration-tui/tests/support/frame_grid.rs"]
pub mod frame_grid;
#[path = "../../agent-collaboration-tui/tests/support/recording_backend.rs"]
pub mod recording_backend;

#[derive(Clone, Default)]
pub(crate) struct PublishedQuotaPopup {
    stack: std::sync::Arc<
        std::sync::Mutex<Option<agent_collaboration_tui::surface_handle::SurfaceStackHandle>>,
    >,
}

impl PublishedQuotaPopup {
    pub(crate) fn observer(
        &self,
    ) -> iocraft::Handler<agent_collaboration_tui::surface_handle::SurfaceStackHandle> {
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
                agent_collaboration_tui::surface_stack::OverlayKind::Dropdown { .. }
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

fn published_popup_frame(canvas: &iocraft::Canvas, cells: &[(u16, u16)]) -> String {
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
