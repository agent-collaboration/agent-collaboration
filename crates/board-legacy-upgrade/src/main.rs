mod import_access;
mod import_command;
use clap::Parser;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // This command's progress is counts and elapsed time only. Do not enable
    // other targets or inherited spans, which may contain imported content.
    let _subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .without_time()
                .with_target(false)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(std::io::stderr)
                .with_filter(
                    tracing_subscriber::filter::Targets::new()
                        .with_target("board_legacy_upgrade::progress", tracing::Level::INFO)
                        .with_target(
                            "message_board_storage::import_progress",
                            tracing::Level::INFO,
                        ),
                ),
        )
        .try_init();
    match import_command::ImportCommand::parse().execute().await {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::from(2),
        Err(error) => {
            eprintln!("upgrade failed: {error}");
            std::process::ExitCode::from(2)
        }
    }
}
