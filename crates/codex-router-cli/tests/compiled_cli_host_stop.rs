#[cfg(not(feature = "keychain-test-support"))]
compile_error!("compiled CLI Host stop tests require keychain-test-support");

use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use codex_router_host::{HostCoordinationPaths, HostInstanceLock, InstanceAcquireError};

#[tokio::test]
async fn stop_absent_host_with_overlong_socket_path_succeeds() {
    for retained_lock_file in [false, true] {
        // Arrange: a valid filesystem path that cannot fit in a Unix socket address.
        let directory = tempfile::tempdir().expect("fixture");
        let root = directory.path().join("long-router-root-".repeat(12));
        let paths = HostCoordinationPaths::for_router_root(&root);
        assert!(std::os::unix::net::SocketAddr::from_pathname(paths.operator_socket()).is_err());
        if retained_lock_file {
            std::fs::create_dir_all(&root).expect("fixture root");
            drop(HostInstanceLock::acquire(&paths).expect("unlocked retained lock file"));
        }

        // Act through the compiled CLI, including its exit-code mapping.
        let output = run_stop(&root).await.expect("compiled CLI stop");

        // Assert: absence is independent of socket address representability.
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(output.stdout, b"Host is not running.\n");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(root.exists(), retained_lock_file);
        assert!(!paths.operator_socket().exists());
    }
}

#[tokio::test]
async fn stop_locked_host_with_overlong_socket_path_reports_named_error() {
    // Arrange: a real owner lock, but no encodable operator socket address.
    let directory = tempfile::tempdir().expect("fixture");
    let root = directory.path().join("long-router-root-".repeat(12));
    let paths = HostCoordinationPaths::for_router_root(&root);
    std::fs::create_dir_all(&root).expect("fixture root");
    let _owner = HostInstanceLock::acquire(&paths).expect("retained owner");
    assert!(std::os::unix::net::SocketAddr::from_pathname(paths.operator_socket()).is_err());

    // Act.
    let output = run_stop(&root).await.expect("compiled CLI stop");

    // Assert: the error names the actual path and leaves ownership untouched.
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let error = String::from_utf8(output.stderr).expect("UTF-8 error");
    assert!(
        error.contains("path is too long for a Unix socket"),
        "{error}"
    );
    assert!(
        error.contains(&paths.operator_socket().display().to_string()),
        "{error}"
    );
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
}

async fn run_stop(root: &Path) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_codex-router"))
            .args(["host", "stop", "--router-root"])
            .arg(root)
            .env("OTEL_SDK_DISABLED", "true")
            .env_remove("CODEX_ROUTER_USE_HOME_DEFAULT")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}
