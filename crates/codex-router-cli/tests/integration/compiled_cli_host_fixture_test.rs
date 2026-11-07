use super::{TestDirectory, binary_version, check, reserve_loopback_port, sign_debug_binary};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn failed_fixture_drop_kills_its_serve_and_preserves_the_neighbor() -> TestResult {
    // Arrange two real signed Serve processes with distinct owned roots.
    let failed = TestDirectory::new()?;
    let neighbor = TestDirectory::new()?;
    let failed_root = failed.path().to_owned();
    let neighbor_root = neighbor.path().to_owned();
    let mut failed_serve = signed_serve(&failed).await?;
    let mut neighbor_serve = signed_serve(&neighbor).await?;

    // Act: unwind the directory owner exactly as an early proof error does.
    let proof: std::io::Result<()> = async move {
        let _directory = failed;
        Err(std::io::Error::other("deliberately failed fixture proof"))
    }
    .await;
    let stopped = tokio::time::timeout(Duration::from_secs(5), failed_serve.wait()).await;
    let result = (|| -> TestResult {
        check(proof.is_err(), "the fixture proof must fail before cleanup")?;
        let status = stopped.map_err(|_| "failed proof leaked its Serve process")??;
        check(!status.success(), "Drop must terminate the owned Serve")?;
        check(
            neighbor_serve.try_wait()?.is_none(),
            "failed fixture cleanup killed the neighboring Serve",
        )
    })();

    // Reap both children even when the absence assertion is red.
    if failed_serve.try_wait()?.is_none() {
        failed_serve.kill().await?;
    }
    drop(neighbor);
    if neighbor_serve.try_wait()?.is_none() {
        neighbor_serve.kill().await?;
    }
    failed_serve.wait().await?;
    neighbor_serve.wait().await?;
    if result.is_ok() {
        // Drop only signaled termination. These actual Child joins authorize
        // removing our exact roots without a process-table polling loop.
        for fixture_root in [failed_root, neighbor_root] {
            match std::fs::remove_dir_all(fixture_root) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    result
}

async fn signed_serve(directory: &TestDirectory) -> Result<Child, Box<dyn std::error::Error>> {
    let binary = directory.path().join("codex-router");
    std::fs::copy(env!("CARGO_BIN_EXE_codex-router"), &binary)?;
    sign_debug_binary(&binary).await?;
    let _version = binary_version(&binary).await?;
    let root = directory.path().join("private-router");
    std::fs::create_dir(&root)?;
    let port = reserve_loopback_port()?;
    let stderr = directory.path().join("serve-stderr.log");
    let mut child = Command::new(&binary)
        .args(["serve", "--port", &port.to_string(), "--state-db"])
        .arg(root.join("state.sqlite"))
        .arg("--secret-root")
        .arg(root.join("secrets"))
        .arg("--require-debug-isolation")
        .env("HOME", directory.path())
        .env("OTEL_SDK_DISABLED", "true")
        .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
        .stdout(Stdio::piped())
        .stderr(Stdio::from(std::fs::File::create(&stderr)?))
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().ok_or("Serve stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await? {
            if line.contains("listening: 127.0.0.1:") {
                return Ok::<(), Box<dyn std::error::Error>>(());
            }
        }
        Err(format!(
            "signed Serve exited before readiness: {}",
            std::fs::read_to_string(&stderr)?
        )
        .into())
    })
    .await
    .map_err(|_| {
        std::io::Error::other(format!(
            "signed fixture Serve did not announce readiness in ten seconds; child={:?}; stderr={}",
            child.try_wait(),
            std::fs::read_to_string(&stderr).unwrap_or_default()
        ))
    })??;
    Ok(child)
}
