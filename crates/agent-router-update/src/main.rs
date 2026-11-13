mod update_command;
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match update_command::RouterUpdateCommand::parse().execute().await {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::from(2),
        Err(error) => {
            eprintln!("upgrade failed: {error}");
            std::process::ExitCode::from(2)
        }
    }
}
