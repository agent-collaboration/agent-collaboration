//! Operator entry point; inspection never takes the running Host's authority.
use board_staging_cutover::{
    CanaryRunReport, ClaimRunReport, CutoverPlanPreview, HostInspection,
    InstalledVerificationReport, OfflineHost, OfflineRunPlan, OfflineRunReport,
};
use std::io::Write;
use std::process::ExitCode;

mod cutover_arguments;
use cutover_arguments::CutoverCommand;

const USAGE: &str = concat!(
    "codex-router-board-staging-cutover inspect --router-root <absolute-path> --service-id <uuid> --peer-id <uuid>\n",
    "codex-router-board-staging-cutover offline --plan <private-file> (--owner-credential-file <private-file> | --owner-access)\n",
    "codex-router-board-staging-cutover claim --plan <private-file> (--owner-credential-file <private-file> | --owner-access)\n",
    "codex-router-board-staging-cutover verify --plan <private-file>\n",
    "codex-router-board-staging-cutover canary --plan <private-file>\n",
    "Plan commands accept --dry-run (no authentication required).\n",
    "Production: add --production --target <central-origin>, then --confirm-production <dry-run-digest> for execution.\n",
);

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HelperOutput {
    CutoverPlan {
        #[serde(flatten)]
        preview: CutoverPlanPreview,
    },
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
    ProjectsClaimed {
        #[serde(flatten)]
        report: ClaimRunReport,
    },
    ClaimsIncomplete {
        #[serde(flatten)]
        report: ClaimRunReport,
    },
    ContentVerified {
        #[serde(flatten)]
        report: InstalledVerificationReport,
    },
    ContentUnverified {
        #[serde(flatten)]
        report: InstalledVerificationReport,
    },
    CanariesVerified {
        #[serde(flatten)]
        report: CanaryRunReport,
    },
    CanariesUnverified {
        #[serde(flatten)]
        report: CanaryRunReport,
    },
}

async fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    match CutoverCommand::parse(std::env::args_os().skip(1))? {
        CutoverCommand::Help => std::io::stdout().lock().write_all(USAGE.as_bytes())?,
        CutoverCommand::Preview {
            plan,
            production_target,
        } => {
            let plan = OfflineRunPlan::read(&plan)?;
            let home = owner_home(plan.router_root())?;
            let preview = plan.dry_run(&home, production_target.as_ref())?;
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &HelperOutput::CutoverPlan { preview })?;
            stdout.write_all(b"\n")?;
        }
        CutoverCommand::Inspect(request) => {
            // The OS account owns protected roots; an overridden HOME grants nothing.
            let home = owner_home(&request.router_root)?;
            let host = OfflineHost::inspect(
                &request.router_root,
                &request.service_id,
                request.peer_id,
                &home,
            )?;
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &HelperOutput::HostInspected { host })?;
            stdout.write_all(b"\n")?;
        }
        CutoverCommand::Offline {
            plan,
            authentication,
            mode,
        } => {
            let plan = mode.apply(OfflineRunPlan::read(&plan)?)?;
            let home = owner_home(plan.router_root())?;
            let report = plan.execute(&home, authentication).await?;
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
        CutoverCommand::Canary { plan, mode } => {
            let plan = mode.apply(OfflineRunPlan::read(&plan)?)?;
            let home = owner_home(plan.router_root())?;
            let report = plan.run_canaries(&home).await?;
            let complete = report.all_verified();
            let output = if complete {
                HelperOutput::CanariesVerified { report }
            } else {
                HelperOutput::CanariesUnverified { report }
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
        CutoverCommand::Verify { plan, mode } => {
            let plan = mode.apply(OfflineRunPlan::read(&plan)?)?;
            let home = owner_home(plan.router_root())?;
            let report = plan.verify_installed_projects(&home).await?;
            let complete = report.all_verified();
            let output = if complete {
                HelperOutput::ContentVerified { report }
            } else {
                HelperOutput::ContentUnverified { report }
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
        CutoverCommand::Claim {
            plan,
            authentication,
            mode,
        } => {
            let plan = mode.apply(OfflineRunPlan::read(&plan)?)?;
            let home = owner_home(plan.router_root())?;
            let report = plan.claim_imported_projects(&home, authentication).await?;
            let complete = report.all_claimed();
            let output = if complete {
                HelperOutput::ProjectsClaimed { report }
            } else {
                HelperOutput::ClaimsIncomplete { report }
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

fn owner_home(
    router_root: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if !router_root.is_absolute() {
        return Err("the declared Router root must be absolute".into());
    }
    let owner = nix::unistd::User::from_uid(nix::unistd::geteuid())?
        .ok_or("cannot determine the Host owner's home directory")?;
    // This rehearsal seam is absent from release builds. HOME never selects authority.
    #[cfg(debug_assertions)]
    if let Some(home) = std::env::var_os("CODEX_ROUTER_CUTOVER_TEST_HOME") {
        use std::os::unix::fs::MetadataExt;
        let home = std::path::PathBuf::from(home);
        if !home.is_absolute() {
            return Err("debug cutover home must be an absolute private fixture directory".into());
        }
        let metadata = std::fs::symlink_metadata(&home)?;
        let home = std::fs::canonicalize(home)?;
        let actual_home = std::fs::canonicalize(&owner.dir)?;
        // An override must never turn the real account's protected roots into staging roots.
        let protected = |root: &std::path::Path| {
            [".codex", ".codex-router", ".codex-router-debug"]
                .iter()
                .any(|name| root.starts_with(actual_home.join(name)))
        };
        if protected(router_root) || protected(&std::fs::canonicalize(router_root)?) {
            return Err("debug cutover home cannot authorize a protected OS-account root".into());
        }
        if !metadata.is_dir()
            || metadata.mode() & 0o077 != 0
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || home == actual_home
            || [".codex", ".codex-router", ".codex-router-debug"]
                .iter()
                .any(|name| home.starts_with(actual_home.join(name)))
        {
            return Err("debug cutover home must be an isolated private fixture directory".into());
        }
        return Ok(home);
    }
    Ok(owner.dir)
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
