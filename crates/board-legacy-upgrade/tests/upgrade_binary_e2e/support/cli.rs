use super::{E2eResult, promote_error};
use crate::upload_support::actual_cli::{ImporterCliInvocation, run_importer_cli};
use collaboration_central_conformance::owner_assertion;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;

pub async fn run_cli(
    source: &Path,
    router_root: &Path,
    report: &Path,
    central_url: Option<&str>,
    owner: &str,
    machine: &str,
) -> E2eResult<Output> {
    let source_copies = [source];
    let cli_binary = PathBuf::from(env!("CARGO_BIN_EXE_codex-router-board-upgrade"));
    let signing_runner = cli_runner_path();
    let assertion = central_url
        .map(|_| owner_assertion())
        .transpose()
        .map_err(promote_error)?;
    run_importer_cli(ImporterCliInvocation {
        cli_binary: &cli_binary,
        signing_runner: &signing_runner,
        source_copies: &source_copies,
        router_directory: router_root,
        report_path: report,
        central_url,
        access_assertion: assertion.as_deref(),
        owner_human_id: owner,
        machine_label: machine,
    })
    .await
}

fn cli_runner_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh")
}

pub fn read_report(path: &Path) -> E2eResult<Value> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
