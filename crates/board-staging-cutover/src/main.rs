//! Operator entry point; inspection never takes the running Host's authority.
use board_staging_cutover::{HostInspection, OfflineHost, OfflineRunPlan, OfflineRunReport};
use std::io::Write;
use std::process::ExitCode;

mod cutover_arguments;
use cutover_arguments::CutoverCommand;

const USAGE: &str = "codex-router-board-staging-cutover inspect --router-root <absolute-path> --service-id <uuid> --peer-id <uuid>\ncodex-router-board-staging-cutover offline --plan <private-file> (--owner-credential-file <private-file> | --owner-access)\n";

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HelperOutput {
    HostInspected {
        #[serde(flatten)]
        host: HostInspection,
    },
    OfflineCompleted {
        #[serde(flatten)]
        report: OfflineRunReport,
    },
    OfflineIncomplete {
        #[serde(flatten)]
        report: OfflineRunReport,
    },
}

async fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    match CutoverCommand::parse(std::env::args_os().skip(1))? {
        CutoverCommand::Help => std::io::stdout().lock().write_all(USAGE.as_bytes())?,
        CutoverCommand::Inspect(request) => {
            // The OS account owns protected roots; an overridden HOME grants nothing.
            let owner = nix::unistd::User::from_uid(nix::unistd::geteuid())?
                .ok_or("cannot determine the Host owner's home directory")?;
            let host = OfflineHost::inspect(
                &request.router_root,
                &request.service_id,
                request.peer_id,
                &owner.dir,
            )?;
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &HelperOutput::HostInspected { host })?;
            stdout.write_all(b"\n")?;
        }
        CutoverCommand::Offline {
            plan,
            authentication,
        } => {
            let plan = OfflineRunPlan::read(&plan)?;
            let owner = nix::unistd::User::from_uid(nix::unistd::geteuid())?
                .ok_or("cannot determine the Host owner's home directory")?;
            let report = plan.execute(&owner.dir, authentication).await?;
            let complete = report.all_ready();
            let output = if complete {
                HelperOutput::OfflineCompleted { report }
            } else {
                HelperOutput::OfflineIncomplete { report }
            };
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &output)?;
            stdout.write_all(b"\n")?;
            return Ok(if complete {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
