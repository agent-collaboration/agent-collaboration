//! Raw taffy grid through iocraft's existing updater. The exact dev dependency
//! enables grid on the same taffy instance; no iocraft source or pin is changed.
use iocraft::taffy::{
    Display, Style,
    geometry::Size as GridSize,
    style_helpers::{fr, length},
};
use iocraft::{Component, ComponentDrawer, ComponentUpdater, Hook, prelude::*};
use std::{
    collections::BTreeMap,
    fs, io,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedCell {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}
type CellRecorder = Arc<Mutex<BTreeMap<String, ObservedCell>>>;
struct RecordCellLayout {
    label: String,
    recorder: CellRecorder,
}
impl Hook for RecordCellLayout {
    fn post_component_draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        let position = drawer.canvas_position();
        let size = drawer.size();
        if let Ok(mut recorder) = self.recorder.lock() {
            recorder.insert(
                self.label.clone(),
                ObservedCell {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
            );
        }
    }
}
#[derive(Props)]
struct GridCellProps {
    label: String,
    #[iocraft(required)]
    recorder: CellRecorder,
}
#[component]
fn GridCell(props: &GridCellProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let label = props.label.clone();
    let recorder = props.recorder.clone();
    hooks.use_hook(move || RecordCellLayout { label, recorder });
    // Let grid stretch the automatic width. A 100% width control overflows
    // fractional tracks at this pin; keep height unchanged for that comparison.
    element!(View(height: 100pct, border_style: BorderStyle::Single) { Text(content: props.label.clone()) })
}
#[derive(Props)]
struct RawGridProps<'a> {
    width: u16,
    children: Vec<AnyElement<'a>>,
}
struct RawGrid;
impl Component for RawGrid {
    type Props<'a> = RawGridProps<'a>;
    fn new(_props: &Self::Props<'_>) -> Self {
        Self
    }
    fn update(
        &mut self,
        props: &mut Self::Props<'_>,
        _hooks: Hooks,
        updater: &mut ComponentUpdater,
    ) {
        updater.set_layout_style(Style {
            display: Display::Grid,
            size: GridSize {
                width: length(f32::from(props.width)),
                height: length(9.0_f32),
            },
            grid_template_columns: vec![fr(1.0_f32), length(12.0_f32), fr(2.0_f32)],
            grid_template_rows: vec![length(3.0_f32); 3],
            ..Style::default()
        });
        updater.update_children(props.children.iter_mut(), None);
    }
}
fn main() -> io::Result<()> {
    fs::create_dir_all("tmp/probes/8")?;
    for (width, columns) in [
        (60_u16, [(0_i16, 16_u16), (16, 12), (28, 32)]),
        (90, [(0, 26), (26, 12), (38, 52)]),
    ] {
        let recorder = CellRecorder::default();
        let mut scene = element!(RawGrid(width) {
            #((0..9).map(|index| element!(GridCell(label: format!("R{}C{}", index / 3 + 1, index % 3 + 1), recorder: recorder.clone()))))
        });
        let canvas = scene.render(Some(usize::from(width)));
        let observations = recorder
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let mut report = format!(
            "GRID width={width} columns=[1fr,12,2fr] rows=[3,3,3]\n{canvas}\nALL_LAYOUTS={observations:?}\n"
        );
        for (row, y) in [(1, 0_i16), (2, 3), (3, 6)] {
            for (column, (x, cell_width)) in columns.into_iter().enumerate() {
                let label = format!("R{row}C{}", column + 1);
                let expected = ObservedCell {
                    x,
                    y,
                    width: cell_width,
                    height: 3,
                };
                let actual = observations.get(&label);
                report.push_str(&format!(
                    "{label}: expected={expected:?} actual={actual:?}\n"
                ));
                if actual != Some(&expected) {
                    return Err(io::Error::other(format!(
                        "grid geometry mismatch: {report}"
                    )));
                }
                let text_x = usize::try_from(x + 1).map_err(io::Error::other)?;
                let text_y = usize::try_from(y + 1).map_err(io::Error::other)?;
                if canvas.cell(text_x, text_y).and_then(CanvasCell::text) != Some("R") {
                    return Err(io::Error::other(format!(
                        "grid label missing at ({text_x},{text_y})"
                    )));
                }
            }
        }
        println!("{report}");
        fs::write(format!("tmp/probes/8/grid-{width}.txt"), report)?;
        let mut ansi = Vec::new();
        canvas.write_ansi(&mut ansi)?;
        fs::write(format!("tmp/probes/8/grid-{width}.ansi"), ansi)?;
    }
    Ok(())
}
