//! Actual CLI publication into a caller-owned, already-running central.
use codex_router_test_support::{FixtureResult, owned_cli_process::run_cli_output};
use message_board::ProjectId;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    os::unix::fs::OpenOptionsExt as _,
    path::Path,
    process::Output,
    time::Duration,
};
use tokio::process::Command;

const CLI_TIMEOUT: Duration = Duration::from_secs(45);
const EXPECTED_PROJECT_IDS: [&str; 3] = [
    "019a0000-0000-7000-8000-000000000015",
    "019a0000-0000-7000-8000-000000000016",
    "019a0000-0000-7000-8000-000000000017",
];
pub const CLAIMABLE_PROJECT_ID: &str = "019a0000-0000-7000-8000-000000000015";
pub const UNHELD_PROJECT_ID: &str = "019a0000-0000-7000-8000-000000000016";

/// Caller-owned inputs for one actual CLI publication into an existing central.
pub struct PublishLegacyFixtureRequest<'a> {
    pub cli_binary: &'a Path,
    pub signing_runner: &'a Path,
    pub source_copy: &'a Path,
    pub router_directory: &'a Path,
    pub report_path: &'a Path,
    pub central_url: &'a str,
    pub access_assertion: &'a str,
    pub owner_human_id: &'a str,
    pub machine_label: &'a str,
}

/// Literal imported IDs and the CLI report from the actual publication.
pub struct PublishedLegacyFixture {
    pub project_ids: Vec<ProjectId>,
    pub claimable_project: ProjectId,
    pub unheld_project: ProjectId,
    pub report: Value,
}

/// Inputs for the shared actual-CLI process owner, including dry-run calls.
pub struct ImporterCliInvocation<'a> {
    pub cli_binary: &'a Path,
    pub signing_runner: &'a Path,
    pub source_copies: &'a [&'a Path],
    pub router_directory: &'a Path,
    pub report_path: &'a Path,
    pub central_url: Option<&'a str>,
    pub access_assertion: Option<&'a str>,
    pub owner_human_id: &'a str,
    pub machine_label: &'a str,
}

/// Runs one or more real importer CLI sources through the owned child process helper.
pub async fn run_importer_cli(invocation: ImporterCliInvocation<'_>) -> FixtureResult<Output> {
    validate_invocation(&invocation)?;
    let assertion_file = match (invocation.central_url, invocation.access_assertion) {
        (Some(_), Some(assertion)) => {
            Some(write_access_assertion(invocation.report_path, assertion)?)
        }
        (Some(_), None) | (None, None) => None,
        (None, Some(_)) => return Err("Access assertion requires a central URL".into()),
    };
    let output = run_importer_cli_process(&invocation, assertion_file.as_deref()).await;
    match assertion_file {
        Some(path) => retain_cleanup_result(output, fs::remove_file(path)),
        None => output,
    }
}

/// Publishes the three-project checked-in fixture through the actual CLI.
/// The caller supplies the live central and owns all paths and cleanup.
pub async fn publish_legacy_fixture_to_central(
    request: PublishLegacyFixtureRequest<'_>,
) -> FixtureResult<PublishedLegacyFixture> {
    validate_publication_request(&request)?;
    let source_copies = [request.source_copy];
    let output = run_importer_cli(ImporterCliInvocation {
        cli_binary: request.cli_binary,
        signing_runner: request.signing_runner,
        source_copies: &source_copies,
        router_directory: request.router_directory,
        report_path: request.report_path,
        central_url: Some(request.central_url),
        access_assertion: Some(request.access_assertion),
        owner_human_id: request.owner_human_id,
        machine_label: request.machine_label,
    })
    .await?;
    require_success(&output)?;
    let report: Value = serde_json::from_slice(&fs::read(request.report_path)?)?;
    let project_ids = imported_project_ids(&report)?;
    let expected_project_ids = EXPECTED_PROJECT_IDS.map(str::to_owned);
    let mut actual_ids = project_ids
        .iter()
        .map(|project| project.as_str().to_owned())
        .collect::<Vec<_>>();
    actual_ids.sort();
    let mut expected_ids = expected_project_ids.to_vec();
    expected_ids.sort();
    if actual_ids != expected_ids {
        return Err(format!("legacy CLI imported unexpected project IDs: {actual_ids:?}").into());
    }
    let mut report_ids = project_report_ids(&report)?
        .iter()
        .map(|project| project.as_str().to_owned())
        .collect::<Vec<_>>();
    report_ids.sort();
    if report_ids != expected_ids {
        return Err(format!("legacy CLI report has unexpected project IDs: {report_ids:?}").into());
    }
    Ok(PublishedLegacyFixture {
        project_ids,
        claimable_project: ProjectId::try_from(CLAIMABLE_PROJECT_ID.to_owned())?,
        unheld_project: ProjectId::try_from(UNHELD_PROJECT_ID.to_owned())?,
        report,
    })
}

async fn run_importer_cli_process(
    invocation: &ImporterCliInvocation<'_>,
    assertion_file: Option<&Path>,
) -> FixtureResult<Output> {
    let mut command = Command::new(invocation.signing_runner);
    command
        .arg(invocation.cli_binary)
        .arg("--router-directory")
        .arg(invocation.router_directory)
        .arg("--report")
        .arg(invocation.report_path)
        .args([
            "--owner-human-id",
            invocation.owner_human_id,
            "--machine-label",
            invocation.machine_label,
        ]);
    for source_copy in invocation.source_copies {
        command.arg("--source").arg(source_copy);
    }
    if let Some(central_url) = invocation.central_url {
        command.arg("--central-url").arg(central_url);
        if let Some(assertion_file) = assertion_file {
            command.env(
                "CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE",
                assertion_file,
            );
        }
    } else {
        command.arg("--dry-run");
    }
    run_cli_output(command, CLI_TIMEOUT).await
}

fn validate_publication_request(request: &PublishLegacyFixtureRequest<'_>) -> FixtureResult {
    let source_copies = [request.source_copy];
    validate_invocation(&ImporterCliInvocation {
        cli_binary: request.cli_binary,
        signing_runner: request.signing_runner,
        source_copies: &source_copies,
        router_directory: request.router_directory,
        report_path: request.report_path,
        central_url: Some(request.central_url),
        access_assertion: Some(request.access_assertion),
        owner_human_id: request.owner_human_id,
        machine_label: request.machine_label,
    })?;
    if request.report_path.exists() {
        return Err("publication report path must be fresh".into());
    }
    Ok(())
}

fn validate_invocation(invocation: &ImporterCliInvocation<'_>) -> FixtureResult {
    for (label, path) in [
        ("CLI binary", invocation.cli_binary),
        ("signing runner", invocation.signing_runner),
        ("Router directory", invocation.router_directory),
        ("report path", invocation.report_path),
    ] {
        if !path.is_absolute() {
            return Err(format!("{label} path must be absolute: {path:?}").into());
        }
    }
    if !invocation.cli_binary.is_file()
        || !invocation.signing_runner.is_file()
        || !invocation.router_directory.is_dir()
    {
        return Err("actual CLI input path is missing or has the wrong kind".into());
    }
    if invocation.source_copies.is_empty()
        || invocation
            .source_copies
            .iter()
            .any(|path| !path.is_absolute() || !path.is_file())
    {
        return Err("actual CLI source copies must be absolute existing files".into());
    }
    let report_parent = invocation
        .report_path
        .parent()
        .ok_or("report path has no parent directory")?;
    if !report_parent.is_dir() {
        return Err("report parent directory must exist".into());
    }
    if invocation
        .central_url
        .is_some_and(|url| url.trim().is_empty())
        || invocation
            .access_assertion
            .is_some_and(|assertion| assertion.trim().is_empty())
    {
        return Err("existing central URL and Access assertion must be nonempty".into());
    }
    Ok(())
}

fn write_access_assertion(
    report_path: &Path,
    assertion: &str,
) -> FixtureResult<std::path::PathBuf> {
    let path = report_path
        .parent()
        .ok_or("report path has no parent directory")?
        .join("central-assertion.txt");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let write_result = file.write_all(assertion.trim().as_bytes());
    drop(file);
    if let Err(write_error) = write_result {
        let cleanup_result = fs::remove_file(&path);
        return Err(combine_cleanup_error(write_error, cleanup_result).into());
    }
    Ok(path)
}

fn retain_cleanup_result(
    output: FixtureResult<Output>,
    cleanup: std::io::Result<()>,
) -> FixtureResult<Output> {
    match (output, cleanup) {
        (Ok(output), Ok(())) => Ok(output),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error.into()),
        (Err(error), Err(cleanup_error)) => Err(std::io::Error::other(format!(
            "CLI observation failed: {error}; Access assertion cleanup failed: {cleanup_error}"
        ))
        .into()),
    }
}

fn combine_cleanup_error(
    write_error: std::io::Error,
    cleanup: std::io::Result<()>,
) -> std::io::Error {
    match cleanup {
        Ok(()) => write_error,
        Err(cleanup_error) => std::io::Error::other(format!(
            "Access assertion write failed: {write_error}; cleanup failed: {cleanup_error}"
        )),
    }
}

fn require_success(output: &Output) -> FixtureResult {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "actual board-upgrade CLI failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )
    .into())
}

fn imported_project_ids(report: &Value) -> FixtureResult<Vec<ProjectId>> {
    if report.get("kind").and_then(Value::as_str) != Some("uploaded") {
        return Err("actual CLI report is not an uploaded result".into());
    }
    let publications = report
        .pointer("/publication/projects")
        .and_then(Value::as_array)
        .ok_or("actual CLI report has no publication projects")?;
    if publications.len() != EXPECTED_PROJECT_IDS.len() {
        return Err(format!(
            "actual CLI published {} projects; expected {}",
            publications.len(),
            EXPECTED_PROJECT_IDS.len()
        )
        .into());
    }
    publications
        .iter()
        .map(|publication| {
            if publication.pointer("/answer/kind").and_then(Value::as_str) != Some("imported") {
                return Err("synthetic fixture project was not newly imported".into());
            }
            let id = publication
                .get("project")
                .and_then(Value::as_str)
                .ok_or("actual CLI publication omitted a project ID")?;
            Ok(ProjectId::try_from(id.to_owned())?)
        })
        .collect()
}

fn project_report_ids(report: &Value) -> FixtureResult<Vec<ProjectId>> {
    report
        .pointer("/report/projects")
        .and_then(Value::as_array)
        .ok_or("actual CLI report has no project reports")?
        .iter()
        .map(|project| {
            let id = project
                .get("project")
                .and_then(Value::as_str)
                .ok_or("actual CLI project report omitted its project ID")?;
            Ok(ProjectId::try_from(id.to_owned())?)
        })
        .collect()
}
