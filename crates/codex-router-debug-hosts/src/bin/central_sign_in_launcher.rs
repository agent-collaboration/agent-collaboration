//! Dev-only native sign-in fixture IPC over the real private Unix link.
#![forbid(unsafe_code)]

#[path = "central_sign_in_launcher/command_protocol.rs"]
mod command_protocol;
#[path = "central_sign_in_launcher/launch_arguments.rs"]
mod launch_arguments;
#[path = "central_sign_in_launcher/launcher_failure.rs"]
mod launcher_failure;
#[path = "central_sign_in_launcher/shutdown_signals.rs"]
mod shutdown_signals;
#[path = "central_sign_in_launcher/stdin_commands.rs"]
mod stdin_commands;

use collaboration_central_tokio::{OrganizationSignInLink, TokioWorkspaceHost};
use launch_arguments::LaunchConfiguration;
use launcher_failure::{LauncherFailure, LauncherResult};
use router_clock::ManualClock;
use serde::Serialize;
use shutdown_signals::ShutdownSignals;
use std::{io::Write, path::Path, process::ExitCode, sync::Arc};
use stdin_commands::StdinCommands;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Readiness<'socket> {
    base_url: &'static str,
    socket: &'socket Path,
}

fn publish_readiness(socket: &Path) -> LauncherResult {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(
        &mut output,
        &Readiness {
            base_url: "http://localhost",
            socket,
        },
    )
    .map_err(|_| LauncherFailure::WriteReadiness)?;
    writeln!(output).map_err(|_| LauncherFailure::WriteReadiness)?;
    output.flush().map_err(|_| LauncherFailure::WriteReadiness)
}

async fn launch() -> LauncherResult {
    // A signal arriving during setup is queued before readiness is published.
    let mut signals = ShutdownSignals::register()?;
    let configuration = LaunchConfiguration::from_process()?;
    let clock = Arc::new(
        ManualClock::starting_at(configuration.fixed_time)
            .map_err(|_| LauncherFailure::InvalidValue("fixed-time-ms"))?,
    );
    let host = Arc::new(
        TokioWorkspaceHost::open_with_signin(
            &configuration.root,
            clock.clone(),
            configuration.trust,
        )
        .await
        .map_err(|_| LauncherFailure::OpenHost)?,
    );
    let link = match OrganizationSignInLink::start(host.clone(), &configuration.socket) {
        Ok(link) => link,
        Err(_) => {
            host.close().await;
            return Err(LauncherFailure::OpenLink);
        }
    };
    let mut input = match StdinCommands::start() {
        Ok(input) => input,
        Err(failure) => {
            let _ = link.shutdown().await;
            host.close().await;
            return Err(failure);
        }
    };
    let outcome: LauncherResult = async {
        publish_readiness(&configuration.socket)?;
        let mut stdin_open = true;
        loop {
            let line = tokio::select! {
                biased;
                () = signals.wait() => break,
                line = input.next_line(), if stdin_open => line,
            };
            let Some(line) = line else {
                // EOF ends the command stream, preserving the historical server lifetime.
                stdin_open = false;
                continue;
            };
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let answer = tokio::select! {
                biased;
                () = signals.wait() => break,
                answer = command_protocol::answer(&host, &clock, &line) => answer,
            };
            command_protocol::write_answer(&answer)?;
        }
        Ok(())
    }
    .await;
    // Stop and join the reader even if stdin is still open or mid-line.
    let input_closed = input.shutdown().await;
    let link_closed = link
        .shutdown()
        .await
        .map_err(|_| LauncherFailure::CloseLink);
    host.close().await;
    outcome?;
    input_closed?;
    link_closed?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match launch().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            // Only closed classifications reach diagnostics, never error Debug or IPC data.
            eprintln!("central launcher failed: {failure}");
            ExitCode::FAILURE
        }
    }
}
