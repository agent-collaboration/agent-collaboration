//! UI-only cell/style access and panel crops; backend parity shares just the decoder.
use super::frame_grid::{FrameCell, FrameGrid};
impl FrameGrid {
    pub fn cell(&self, column: usize, row: usize) -> Option<&FrameCell> {
        self.cells.get(row)?.get(column)
    }
    /// These component fixtures contain exactly one round framed panel.
    pub fn round_frame_text(&self) -> String {
        let (top, left) = self
            .cells
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| cell.text == "╭")
                    .map(|column| (row, column))
            })
            .expect("round panel's top-left corner");
        let right = self.cells[top]
            .iter()
            .rposition(|cell| cell.text == "╮")
            .expect("round panel's top-right corner");
        let bottom = (top..self.height)
            .find(|row| self.cells[*row][left].text == "╰")
            .expect("round panel's bottom-left corner");
        (top..=bottom)
            .map(|row| {
                self.cells[row][left..=right]
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
