//! Synthetic composition probe; it does not establish the cause of an earlier capture.
use iocraft::prelude::*;
use std::{fs, io, path::Path};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy)]
struct OverlayCase<'a> {
    name: &'a str,
    text: &'a str,
    left: i32,
    opaque: bool,
}

fn render_case(case: OverlayCase<'_>, open: bool) -> Canvas {
    element! {
        View(width: 20, height: 5) {
            Text(content: case.text)
            #(open.then(|| element! {
                View(position: Position::Absolute, left: case.left, top: 0,
                    width: 10, height: 4, border_style: BorderStyle::Single,
                    background_color: case.opaque.then_some(Color::Reset)) {}
            }))
        }
    }
    .render(Some(20))
}

#[component]
fn LiveOverlay(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut phase = hooks.use_state(|| 0usize);
    let mut should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();
    hooks.use_terminal_events(move |event| {
        if let TerminalEvent::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            if key.code == KeyCode::Char('q') {
                should_exit.set(true);
            } else {
                phase += 1;
            }
        }
    });
    if should_exit.get() {
        system.exit();
    }
    element! {
        View(width: 40, height: 10, flex_direction: FlexDirection::Column) {
            Text(content: format!("PHASE {} (key advances; q exits)", phase.get()))
            View(width: 20, height: 5) {
                Text(content: "AA界ZZ")
                #(if phase.get() % 3 != 1 { Some(element! {
                    View(position: Position::Absolute, left: 3, top: 0,
                        width: 10, height: 4, border_style: BorderStyle::Single,
                        background_color: Color::Reset) {}
                }) } else { None })
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    if std::env::args().any(|arg| arg == "--live") {
        return element!(LiveOverlay).render_loop().fullscreen().await;
    }
    let output = Path::new("tmp/probes/1");
    fs::create_dir_all(output)?;
    for glyph in ["X", "界", "✉", "✉️", "📋", "⎇", "📂"] {
        println!("GLYPH {glyph:?} unicode_width={}", glyph.width());
        let left_text = format!("AA{glyph}ZZ");
        // Right edge of overlay is x=12, outside starts x=13.
        let right_text = format!("ABCDEFGHIJKL{glyph}ZZ");
        for (label, text, left, opaque) in [
            ("left-cross", left_text.as_str(), 3, true),
            ("left-start", left_text.as_str(), 2, true),
            ("transparent", left_text.as_str(), 3, false),
            ("right-cross", right_text.as_str(), 3, true),
        ] {
            let name = format!(
                "{}-{label}",
                glyph
                    .chars()
                    .map(|ch| format!("{:x}", u32::from(ch)))
                    .collect::<Vec<_>>()
                    .join("-")
            );
            let case = OverlayCase {
                name: &name,
                text,
                left,
                opaque,
            };
            for (phase, open) in [("open", true), ("close", false), ("reopen", true)] {
                let canvas = render_case(case, open);
                let mut ansi = Vec::new();
                canvas.write_ansi(&mut ansi)?;
                fs::write(output.join(format!("{}-{phase}.ansi", case.name)), &ansi)?;
                println!(
                    "CASE {} phase={phase} canvas(3,0)={:?} canvas(12,0)={:?}",
                    case.name,
                    canvas.cell(3, 0).and_then(CanvasCell::text),
                    canvas.cell(12, 0).and_then(CanvasCell::text)
                );
                println!("PLAIN {:?}", canvas.to_string());
                println!("ANSI {:?}", String::from_utf8_lossy(&ansi));
                let cells = (0..canvas.height())
                    .map(|row| {
                        (0..canvas.width())
                            .map(|col| {
                                format!(
                                    "({col},{row})={:?}",
                                    canvas.cell(col, row).and_then(CanvasCell::text)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                fs::write(output.join(format!("{}-{phase}.cells", case.name)), cells)?;
            }
        }
    }
    Ok(())
}
