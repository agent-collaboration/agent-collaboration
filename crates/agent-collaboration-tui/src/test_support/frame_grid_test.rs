use super::frame_grid::{FrameError, FrameGrid};
use iocraft::prelude::*;
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
