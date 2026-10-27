#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! The shipped CLI derives the two files from its existing one-root argument.
#![allow(clippy::panic_in_result_fn)]
#[path = "upload_support/central_fixture.rs"]
#[allow(
    dead_code,
    reason = "The sibling importer targets also use the owner client helper."
)]
mod central_fixture;
mod message_support;
mod support;
use message_support::*;
use std::{os::unix::fs::PermissionsExt, path::Path};

fn identity(root: &Path) -> TestResult {
    support::create_host_identity(root)?;
    Ok(())
}

async fn run(
    root: &Path,
    board: &Path,
    report: &Path,
    central: Option<&str>,
) -> TestResult<std::process::Output> {
    let mut command =
        tokio::process::Command::new(env!("CARGO_BIN_EXE_codex-router-board-upgrade"));
    command
        .arg("--source")
        .arg(board)
        .arg("--router-directory")
        .arg(root)
        .arg("--report")
        .arg(report)
        .args([
            "--owner-human-id",
            "fixture-owner",
            "--machine-label",
            "message cutover fixture",
        ])
        .kill_on_drop(true);
    if let Some(central) = central {
        let assertion = root.join("assertion.txt");
        std::fs::write(
            &assertion,
            collaboration_central_conformance::owner_assertion()
                .map_err(|error| error as Box<dyn std::error::Error>)?,
        )?;
        std::fs::set_permissions(&assertion, std::fs::Permissions::from_mode(0o600))?;
        command.arg("--central-url").arg(central).env(
            "CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE",
            assertion,
        );
    } else {
        command.arg("--dry-run");
    }
    Ok(tokio::time::timeout(std::time::Duration::from_secs(45), command.output()).await??)
}

#[tokio::test]
async fn dry_run_copies_messages_only_into_disposable_output_and_keeps_source_untouched()
-> TestResult {
    let (directory, board) = support::fixture_copy("legacy-complete.sqlite")?;
    identity(directory.path())?;
    let stores = codex_router_host::router_collaboration_directory(directory.path());
    let source = stores.join("automation.sqlite");
    seeded_source(&source).await?;
    let before = std::fs::read(&source)?;
    let report = directory.path().join("dry-run.json");
    let output = run(directory.path(), &board, &report, None).await?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(
        value.pointer("/messages/counts/pushes"),
        Some(&serde_json::json!(2))
    );
    assert_eq!(
        value.pointer("/messages/counts/subscriptionProgress"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(std::fs::read(&source)?, before);
    assert!(!stores.join("interaction.sqlite").exists());
    Ok(())
}

#[tokio::test]
async fn actual_cli_upload_copies_and_verifies_messages_before_applying_the_real_drop() -> TestResult
{
    let (directory, board) = support::fixture_copy("legacy-complete.sqlite")?;
    identity(directory.path())?;
    let stores = codex_router_host::router_collaboration_directory(directory.path());
    let source = stores.join("automation.sqlite");
    let destination = stores.join("interaction.sqlite");
    seeded_source(&source).await?;
    let before = history(&destination).await?;
    let central = tempfile::tempdir()?;
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(central.path(), &jwks).await?;
    let observed: TestResult = async {
        let report = directory.path().join("uploaded.json");
        let output = run(directory.path(), &board, &report, Some(server.base_url())).await?;
        assert_eq!(
            output.status.code(),
            Some(0),
            "report: {}\nstdout: {}\nstderr: {}",
            std::fs::read_to_string(&report)?,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(source_tables(&source)?.is_empty());
        let ids = text_rows(&destination, IDS)?;
        assert_eq!(ids.len(), 2);
        assert_eq!(text_rows(&destination, HISTORY)?, before);
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
        assert_eq!(
            value.pointer("/messages/kind"),
            Some(&serde_json::json!("verified"))
        );
        assert_eq!(
            value.pointer("/messages/counts/intentProgress"),
            Some(&serde_json::json!(1))
        );
        let replay = run(directory.path(), &board, &report, Some(server.base_url())).await?;
        assert_eq!(
            replay.status.code(),
            Some(0),
            "report: {}\nstdout: {}\nstderr: {}",
            std::fs::read_to_string(&report)?,
            String::from_utf8_lossy(&replay.stdout),
            String::from_utf8_lossy(&replay.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
        assert_eq!(
            value.pointer("/messages/kind"),
            Some(&serde_json::json!("alreadyMoved"))
        );
        assert_eq!(text_rows(&destination, IDS)?, ids);
        assert_eq!(text_rows(&destination, HISTORY)?, before);
        Ok(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}

#[tokio::test]
async fn missing_thread_refuses_dry_and_real_cli_before_publication_or_message_drop() -> TestResult
{
    let (directory, board) = support::fixture_copy("legacy-complete.sqlite")?;
    identity(directory.path())?;
    let stores = codex_router_host::router_collaboration_directory(directory.path());
    let source = stores.join("automation.sqlite");
    seeded_source(&source).await?;
    activity_push(&source, "019a0000-0000-7000-8000-00000000ffff", 3, 9)?;
    let before = std::fs::read(&source)?;
    let report = directory.path().join("refused.json");
    for central in [None, Some("http://127.0.0.1:1")] {
        let output = run(directory.path(), &board, &report, central).await?;
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
        assert_eq!(
            value.get("reason"),
            Some(&serde_json::json!(
                "message activity translation refused: missing_thread_ranges=1, empty_thread_ranges=0"
            ))
        );
        assert_eq!(std::fs::read(&source)?, before);
        assert!(!stores.join("interaction.sqlite").exists());
    }
    Ok(())
}
