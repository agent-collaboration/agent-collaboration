//! Singleton-owned recovery of actual Unix socket files after a Host crash.
use codex_router_host::{HostCoordinationPaths, HostInstance};
use std::{
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    path::Path,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

macro_rules! ensure {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err($message.into());
        }
    };
}

const ENDPOINTS: [&str; 5] = [
    "control.sock",
    "codex-native.sock",
    "codex-acp.sock",
    "router-sessions/claude-local.sock",
    "router-sessions/cursor-local.sock",
];

fn acquire_runtime() -> TestResult<(tempfile::TempDir, HostInstance)> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.path().join("router-sessions"))?;
    let instance = HostInstance::acquire(HostCoordinationPaths::new(
        root.path().join("operator.sock"),
        root.path().join("host.lock"),
    ))?;
    Ok((root, instance))
}

fn socket_inode(path: &Path) -> TestResult<(u64, u64)> {
    let metadata = std::fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[tokio::test]
async fn singleton_clears_refused_endpoint_socket_files() -> TestResult {
    // Arrange: bind then close real listeners, leaving their stale pathnames.
    let (root, instance) = acquire_runtime()?;
    for name in ENDPOINTS {
        let path = root.path().join(name);
        drop(UnixListener::bind(&path)?);
        ensure!(path.exists(), "stale socket fixture is missing");
    }

    // Act: only the held singleton authority performs recovery.
    instance.clear_stale_endpoint_sockets(root.path()).await?;

    // Assert: each known stale socket can now be bound again.
    for name in ENDPOINTS {
        let path = root.path().join(name);
        ensure!(!path.exists(), format!("stale endpoint survived: {name}"));
        drop(UnixListener::bind(path)?);
    }
    Ok(())
}

#[tokio::test]
async fn singleton_never_removes_a_live_endpoint_listener() -> TestResult {
    let (root, instance) = acquire_runtime()?;
    for name in ENDPOINTS {
        // Arrange: another real listener currently owns this exact endpoint inode.
        let path = root.path().join(name);
        let listener = UnixListener::bind(&path)?;
        let before = socket_inode(&path)?;

        // Act: recovery must reject a successful connect probe.
        let result = instance.clear_stale_endpoint_sockets(root.path()).await;

        // Assert: the live listener's pathname and identity remain intact.
        ensure!(
            matches!(result, Err(error) if error.kind() == std::io::ErrorKind::AddrInUse),
            "live socket probe must stop cleanup"
        );
        ensure!(
            socket_inode(&path)? == before,
            "live listener inode changed"
        );
        drop(listener);
        instance.clear_stale_endpoint_sockets(root.path()).await?;
    }
    Ok(())
}

#[tokio::test]
async fn singleton_leaves_unknown_runtime_entries_untouched() -> TestResult {
    let (root, instance) = acquire_runtime()?;
    let names = [
        "notes.txt",
        "unknown.sock",
        "router-sessions/notes.txt",
        "router-sessions/unknown.sock",
    ];
    for name in names {
        let path = root.path().join(name);
        if path
            .extension()
            .is_some_and(|extension| extension == "sock")
        {
            drop(UnixListener::bind(&path)?);
        } else {
            std::fs::write(&path, b"retain this unknown entry")?;
        }
    }
    let before = names
        .into_iter()
        .map(|name| socket_inode(&root.path().join(name)))
        .collect::<TestResult<Vec<_>>>()?;

    instance.clear_stale_endpoint_sockets(root.path()).await?;

    let after = names
        .into_iter()
        .map(|name| socket_inode(&root.path().join(name)))
        .collect::<TestResult<Vec<_>>>()?;
    ensure!(before == after, "unknown entries changed identity");
    ensure!(
        std::fs::read(root.path().join("notes.txt"))? == b"retain this unknown entry",
        "unknown file content changed"
    );
    Ok(())
}
