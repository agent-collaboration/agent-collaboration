//! Ordinary public-API consumer; every effect records local sample data only.
#[path = "component_showcase/sample_clipping.rs"]
mod sample_clipping;
#[path = "component_showcase/sample_coverage.rs"]
mod sample_coverage;
#[path = "component_showcase/sample_dialogs.rs"]
mod sample_dialogs;
#[path = "component_showcase/sample_model.rs"]
mod sample_model;
#[cfg(all(feature = "test-support", debug_assertions))]
#[path = "component_showcase/sample_readback.rs"]
mod sample_readback;
#[cfg(all(feature = "test-support", debug_assertions))]
#[path = "component_showcase/sample_receipts.rs"]
mod sample_receipts;
#[path = "component_showcase/sample_screen.rs"]
mod sample_screen;
#[path = "component_showcase/screen_actions.rs"]
mod screen_actions;

use iocraft::prelude::*;
use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--help"] {
        println!(
            "component_showcase [--receipt-directory ABSOLUTE_EMPTY_OWNED_DIRECTORY]\nLocal component sample: no Router, providers or account store. Menu and toolbar open the demonstrations."
        );
        return Ok(());
    }
    #[cfg(all(feature = "test-support", debug_assertions))]
    let receipts = sample_receipts::SampleReceipts::from_arguments(&args)?;
    #[cfg(not(all(feature = "test-support", debug_assertions)))]
    if !args.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "receipt support requires a debug test-support build",
        ));
    }
    let (initial_width, initial_height) = crossterm::terminal::size()?;
    #[cfg(all(feature = "test-support", debug_assertions))]
    let mut app =
        element!(sample_screen::Showcase(initial_width,initial_height,receipts: receipts.clone()));
    #[cfg(not(all(feature = "test-support", debug_assertions)))]
    let mut app = element!(sample_screen::Showcase(initial_width, initial_height));
    #[cfg(all(feature = "test-support", debug_assertions))]
    let render = app
        .render_loop()
        .stdout(sample_receipts::SampleOutput::new(receipts.clone()));
    #[cfg(not(all(feature = "test-support", debug_assertions)))]
    let render = app.render_loop();
    render
        .enable_bracketed_paste()
        .fullscreen()
        .ignore_ctrl_c()
        .await?;
    #[cfg(all(feature = "test-support", debug_assertions))]
    if let Some(receipts) = receipts {
        receipts.flush_and_check()?;
    }
    println!("COMPONENT_SHOWCASE_EXIT quit-owner");
    Ok(())
}
