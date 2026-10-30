use super::demo_shell_state::DemoShellState;
use iocraft::prelude::*;
use std::{fs, io, path::Path};

fn press(code: KeyCode, modifiers: KeyModifiers) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, code);
    event.modifiers = modifiers;
    TerminalEvent::Key(event)
}
pub fn owner_scenes(width: u16, height: u16) -> Vec<(&'static str, DemoShellState)> {
    let main = DemoShellState::new(width, height);
    let mut details = main.clone();
    details.handle_event(press(KeyCode::Enter, KeyModifiers::empty()));
    let mut new_session = main.clone();
    new_session.handle_event(press(KeyCode::Char('n'), KeyModifiers::CONTROL));
    new_session.handle_event(press(KeyCode::Down, KeyModifiers::empty()));
    new_session.handle_event(press(KeyCode::Enter, KeyModifiers::empty()));
    vec![
        ("main", main),
        ("details", details),
        ("new-dropdown", new_session),
    ]
}
pub fn capture_owner_frames() -> io::Result<()> {
    use unicode_width::UnicodeWidthStr;
    let output = Path::new("tmp/probes/demo-owner-v2");
    fs::create_dir_all(output)?;
    let mut glyph_note = String::new();
    for (label, glyph) in [
        ("retired-menu", "☰"),
        ("menu", super::demo_shell_theme::MENU_ICON),
    ] {
        let glyph_probe =
            element!(View(width: 8, height: 1) { Text(content: format!("{glyph}M")) })
                .render(Some(8));
        glyph_note.push_str(&format!(
            "{label} {glyph:?} UnicodeWidthStr0.2.2={} pinned0.1.14-M-column={:?}\n",
            glyph.width(),
            (0..8).find(
                |column| glyph_probe.cell(*column, 0).and_then(CanvasCell::text) == Some("M")
            )
        ));
    }
    glyph_note.push_str("Chrome is checked under both tables; these captures do not claim physical terminal/font observation.\n");
    fs::write(output.join("glyph-width-note.txt"), &glyph_note)?;
    println!("{glyph_note}");
    for (width, height) in [(120, 36), (76, 30)] {
        for (name, state) in owner_scenes(width, height) {
            let canvas = crate::render_demo_canvas(state);
            let label = format!("{width}x{height}-{name}");
            fs::write(output.join(format!("{label}.txt")), canvas.to_string())?;
            let mut ansi = Vec::new();
            canvas.write_ansi(&mut ansi)?;
            fs::write(output.join(format!("{label}.ansi")), &ansi)?;
            let mut cells = String::new();
            for y in 0..canvas.height() {
                for x in 0..canvas.width() {
                    if let Some(cell) = canvas.cell(x, y) {
                        cells.push_str(&format!(
                            "({x},{y}) text={:?} bg={:?} style={:?}\n",
                            cell.text(),
                            cell.background_color,
                            cell.text_style()
                        ));
                    }
                }
            }
            fs::write(output.join(format!("{label}.cells")), cells)?;
            println!(
                "CAPTURE {label} canvas={}x{} ansi_bytes={} truecolor={}",
                canvas.width(),
                canvas.height(),
                ansi.len(),
                String::from_utf8_lossy(&ansi).contains("48;2;")
            );
        }
    }
    Ok(())
}
