//! Opt-in real board listeners and stores, without native or provider processes.
//!
//! Run with `<directory> <MCP port> [<private fixture session JSON>]`.
//! Port 0 asks the OS for a free port. Read the published service.json for its URL.
//! CLI clients select this directory with --service-directory. Stop with SIGTERM
//! or Ctrl-C; restart on the same directory to exercise durable board recovery.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs, load_fleet_enrollment};
use collaboration_protocol::ServiceManifest;
#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
#[path = "board-debug-runtime/local_cancel_observation.rs"]
mod local_cancel_observation;
#[cfg(feature = "runtime-test-support")]
#[path = "board-debug-runtime/mail_faults.rs"]
mod mail_faults;
#[path = "board-debug-runtime/shutdown_signals.rs"]
mod shutdown_signals;
use std::{
    error::Error,
    io,
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn validate_debug_directory(directory: &Path, port: u16) -> io::Result<PathBuf> {
    let invalid = || {
        io::Error::other("select a private absolute temporary directory and a nonproduction port")
    };
    if !directory.is_absolute() || port == 8787 {
        return Err(invalid());
    }
    let directory = directory.canonicalize()?;
    let temporary_roots = [
        std::env::temp_dir().canonicalize()?,
        Path::new("/tmp").canonicalize()?,
    ];
    let metadata = directory.metadata()?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || !temporary_roots
            .iter()
            .any(|root| directory.starts_with(root) && &directory != root)
    {
        return Err(invalid());
    }
    Ok(directory)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if !cfg!(debug_assertions) {
        return Err("board-debug-runtime requires a debug build".into());
    }
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [directory, port, extra @ ..] = arguments.as_slice() else {
        return Err("usage: board-debug-runtime <private temporary directory> <MCP port> [<private fixture session JSON>]".into());
    };
    let port: u16 = port.to_str().ok_or("port must be UTF-8")?.parse()?;
    let directory = validate_debug_directory(Path::new(directory), port)?;
    let board_session = match extra {
        [] => None,
        [path] => {
            let path = PathBuf::from(path);
            let root = directory.clone();
            Some(
                tokio::task::spawn_blocking(move || load_fleet_enrollment(&path, &root))
                    .await
                    .map_err(io::Error::other)??,
            )
        }
        _ => return Err("expected at most one private fixture session JSON path".into()),
    };
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?);
    // Keep the normal Codex state path. No native process or stored-session read
    // is requested by this launcher; the native endpoint remains unavailable.
    let mut runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session,
        owner_human_id: None,
        directory: directory.clone(),
        codex_home: home.join(".codex"),
        backend_socket: directory.join("absent-native.sock"),
        mcp_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        native_schema: None,
        peer_registry_directory: Some(directory.join("absent-peer-registry")),
        remote_control_server_name: None,
    })
    .await?;
    #[cfg(feature = "runtime-test-support")]
    if let Err(error) = mail_faults::install(&runtime, &directory).await {
        return match runtime.shutdown().await {
            Ok(()) => Err(error.into()),
            Err(cleanup) => Err(format!("mail fault setup: {error}; cleanup: {cleanup}").into()),
        };
    }
    let manifest: ServiceManifest =
        serde_json::from_slice(&std::fs::read(directory.join("service.json"))?)?;
    let mut shutdown_signals = match shutdown_signals::ShutdownSignals::register_handlers() {
        Ok(signals) => signals,
        Err(error) => {
            return match runtime.shutdown().await {
                Ok(()) => Err(error.into()),
                Err(cleanup) => {
                    Err(format!("shutdown signal setup: {error}; cleanup: {cleanup}").into())
                }
            };
        }
    };
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    let hold_before_serve = directory.join("fixture-hold-before-serve").is_file();
    println!(
        "{}",
        serde_json::json!({"kind":"boardDebugRuntimeReady","directory":directory,
            "mcpUrl":manifest.mcp.url,"nativeBackend":"absent"})
    );
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    let outcome = if hold_before_serve {
        println!(
            "{}",
            serde_json::json!({"kind":"boardDebugRuntimeBeforeServeHeld","directory":directory})
        );
        // Stay before query-loop entry until the real, already-armed signal
        // owner receives shutdown. No stdin EOF or elapsed delay releases it.
        wait_for_shutdown(&mut runtime, &mut shutdown_signals).await
    } else {
        local_cancel_observation::serve_until_shutdown(&mut runtime, &mut shutdown_signals).await
    };
    #[cfg(not(all(debug_assertions, feature = "runtime-test-support")))]
    let outcome = wait_for_shutdown(&mut runtime, &mut shutdown_signals).await;
    let shutdown = runtime.shutdown().await;
    outcome?;
    shutdown?;
    Ok(())
}

async fn wait_for_shutdown(
    runtime: &mut CollaborationRuntime,
    shutdown_signals: &mut shutdown_signals::ShutdownSignals,
) -> io::Result<()> {
    tokio::select! {
        result = shutdown_signals.wait_for_signal() => result,
        error = runtime.listener_failure() => Err(io::Error::other(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn launcher_refuses_nonprivate_or_nontemporary_state() -> Result<(), Box<dyn Error>> {
        assert!(validate_debug_directory(Path::new("/"), 0).is_err());
        assert!(validate_debug_directory(Path::new("."), 0).is_err());
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))?;
        assert!(validate_debug_directory(directory.path(), 0).is_err());
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        assert_eq!(
            validate_debug_directory(directory.path(), 0)?,
            directory.path().canonicalize()?
        );
        Ok(())
    }

    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn launcher_refuses_the_production_port() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        assert!(validate_debug_directory(directory.path(), 8787).is_err());
        assert!(validate_debug_directory(directory.path(), 0).is_ok());
        Ok(())
    }
}
