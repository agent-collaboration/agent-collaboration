//! Owned Git checkout and public repository association used by process scenarios.
use super::fixture::{BoardFixture, accepted, at};
use codex_router_test_support::{FixtureResult, owned_cli_process::run_cli_output};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub async fn attach_repository(fixture: &BoardFixture) -> FixtureResult<PathBuf> {
    let repository = fixture.root.path().join("owned-repository");
    std::fs::create_dir(&repository)?;
    let mut command = tokio::process::Command::new("git");
    command.args(["init", "--quiet"]).current_dir(&repository);
    assert!(
        run_cli_output(command, Duration::from_secs(10))
            .await?
            .status
            .success()
    );
    let reply = repository_cli(
        fixture,
        &repository,
        &[
            "repository".into(),
            "attach".into(),
            "--workspace-id".into(),
            fixture.workspace.clone(),
            "--repository-path".into(),
            repository
                .to_str()
                .ok_or("repository path encoding")?
                .to_owned(),
            "--actor".into(),
            fixture.owner.to_string(),
        ],
    )
    .await?;
    accepted(at(&reply, "/record")?)?;
    Ok(repository)
}

pub async fn repository_cli(
    fixture: &BoardFixture,
    directory: &Path,
    arguments: &[String],
) -> FixtureResult<Value> {
    let mut command =
        super::owned_host::artifact_command(Path::new(env!("CARGO_BIN_EXE_agent-collaboration")))?;
    command
        .args(arguments)
        .arg("--service-directory")
        .arg(&fixture.directory)
        .arg("--json")
        .current_dir(directory);
    let output = run_cli_output(command, Duration::from_secs(20)).await?;
    assert!(
        output.status.success(),
        "repository CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout)?;
    at(&reply, "/result").cloned()
}
