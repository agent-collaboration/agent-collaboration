//! ASCII-only layering, clipping and dimming controls.
use iocraft::prelude::*;
use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug)]
enum DropdownHome {
    Descendant,
    Root,
}
#[derive(Clone, Copy, Debug)]
enum BackdropStyle {
    Normal,
    StyledText,
    OpaqueView,
}

fn dropdown() -> impl Into<AnyElement<'static>> {
    element! {
        View(position: Position::Absolute, left: 8, top: 3, width: 10, height: 5,
            background_color: Color::Reset, border_style: BorderStyle::Single) {
            Text(content: "MENU")
        }
    }
}

fn help_surface() -> impl Into<AnyElement<'static>> {
    element! {
        View(position: Position::Absolute, left: 10, top: 4, width: 10, height: 3,
            background_color: Color::Reset, border_style: BorderStyle::Double) {
            Text(content: "HELP")
        }
    }
}

fn scene(home: DropdownHome, help_on_top: Option<bool>, backdrop: BackdropStyle) -> Canvas {
    element! {
        View(width: 40, height: 12, flex_direction: FlexDirection::Column) {
            #((0..12).map(|row| element! {
                Text(content: format!("BACKGROUND ROW {row:02} ................"),
                    weight: if matches!(backdrop, BackdropStyle::StyledText) { Weight::Light } else { Weight::Normal })
            }))
            #(matches!(backdrop, BackdropStyle::OpaqueView).then(|| element! {
                View(position: Position::Absolute, left: 0, top: 0, width: 40, height: 12,
                    background_color: Color::DarkGrey) {}
            }))
            #(matches!(help_on_top, Some(false)).then(help_surface))
            View(position: Position::Absolute, left: 4, top: 2, width: 12, height: 3,
                overflow: Overflow::Hidden, background_color: Color::Reset) {
                Text(content: "DIALOG BODY")
                #(matches!(home, DropdownHome::Descendant).then(|| element! {
                    View(position: Position::Absolute, left: 4, top: 1, width: 10, height: 5,
                        background_color: Color::Reset, border_style: BorderStyle::Single) {
                        Text(content: "MENU")
                    }
                }))
            }
            #(matches!(home, DropdownHome::Root).then(dropdown))
            #(matches!(help_on_top, Some(true)).then(help_surface))
        }
    }.render(Some(40))
}

fn main() -> io::Result<()> {
    let output = Path::new("tmp/probes/2");
    fs::create_dir_all(output)?;
    for home in [DropdownHome::Descendant, DropdownHome::Root] {
        for help_on_top in [None, Some(true), Some(false)] {
            let label = format!("{home:?}-help-{help_on_top:?}");
            let canvas = scene(home, help_on_top, BackdropStyle::Normal);
            println!("CASE {label}\n{canvas}");
            println!(
                "CELL (8,6)={:?} (11,5)={:?}",
                canvas.cell(8, 6).and_then(CanvasCell::text),
                canvas.cell(11, 5).and_then(CanvasCell::text)
            );
            let mut ansi = Vec::new();
            canvas.write_ansi(&mut ansi)?;
            fs::write(output.join(format!("{label}.ansi")), ansi)?;
        }
    }
    for backdrop in [
        BackdropStyle::Normal,
        BackdropStyle::StyledText,
        BackdropStyle::OpaqueView,
    ] {
        let canvas = scene(DropdownHome::Root, None, backdrop);
        println!(
            "BACKDROP {backdrop:?} text(0,0)={:?} style={:?}\n{canvas}",
            canvas.cell(0, 0).and_then(CanvasCell::text),
            canvas.cell(0, 0).and_then(CanvasCell::text_style)
        );
        let mut ansi = Vec::new();
        canvas.write_ansi(&mut ansi)?;
        fs::write(output.join(format!("backdrop-{backdrop:?}.ansi")), ansi)?;
    }
    Ok(())
}
