//! `agent-router-update`: run a Router root's pending one-time steps, list them with
//! `--dry-run`.
use agent_router_update::{RouterUpdatePlan, plan_router_update, run_router_update};
use clap::Parser;
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "agent-router-update",
    version,
    about = "Run the one-time updates a Router root needs, with its Host stopped"
)]
pub struct RouterUpdateCommand {
    /// The Router root to update, such as ~/.codex-router.
    #[arg(long, required = true)]
    router_root: Option<PathBuf>,
    /// List the pending steps and what each would change, changing nothing.
    #[arg(long)]
    dry_run: bool,
}

impl RouterUpdateCommand {
    /// Returns whether the command succeeded; failures are already reported on stderr.
    pub async fn execute(self) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(root) = self.router_root else {
            return Err("--router-root is required".into());
        };
        let mut stdout = std::io::stdout().lock();
        let result = if self.dry_run {
            plan_router_update(&root).await
        } else {
            run_router_update(&root).await
        };
        match result {
            Ok(plan) => {
                write_plan(&mut stdout, &root, &plan, self.dry_run)?;
                Ok(true)
            }
            Err(error) => {
                eprintln!("agent-router-update: {error}");
                Ok(false)
            }
        }
    }
}

fn write_plan(
    output: &mut impl Write,
    root: &std::path::Path,
    plan: &RouterUpdatePlan,
    dry_run: bool,
) -> std::io::Result<()> {
    if plan.is_empty() {
        return writeln!(output, "{} needs no update", root.display());
    }
    let verb = if dry_run { "pending" } else { "done" };
    for stale in &plan.stale {
        writeln!(output, "{verb} {stale}")?;
    }
    for step in &plan.steps {
        writeln!(output, "{verb} {}: {}", step.step.label(), step.change)?;
    }
    if dry_run {
        writeln!(output, "dry run: nothing was changed")?;
    }
    Ok(())
}
