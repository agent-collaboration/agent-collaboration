//! Opt-in real Host collaboration listeners and stores, without a native process. With the
//! runtime-test-support feature, a `fixture-provider-acp.json` in the directory starts
//! one fixture ACP provider on the Cursor endpoint.
//!
//! Run with `<directory> <MCP port>`.
//! Port 0 asks the OS for a free port. Read the published service.json for its URL.
//! CLI clients select this directory with --service-directory. Stop with SIGTERM
//! or Ctrl-C; restart on the same directory to exercise durable recovery.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_protocol::ServiceManifest;
#[cfg(feature = "runtime-test-support")]
#[path = "board-debug-runtime/fixture_provider.rs"]
mod fixture_provider;
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
    let [directory, port] = arguments.as_slice() else {
        return Err("usage: board-debug-runtime <private temporary directory> <MCP port>".into());
    };
    let port: u16 = port.to_str().ok_or("port must be UTF-8")?.parse()?;
    let directory = validate_debug_directory(Path::new(directory), port)?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?);
    // Keep the normal Codex state path. No native process or stored-session read
    // is requested by this launcher; the native endpoint remains unavailable.
    let inputs = CollaborationRuntimeInputs {
        owner_human_id: None,
        directory: directory.clone(),
        codex_home: home.join(".codex"),
        backend_socket: directory.join("absent-native.sock"),
        mcp_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        native_schema: None,
        peer_registry_directory: Some(directory.join("absent-peer-registry")),
        remote_control_server_name: None,
    };
    #[cfg(feature = "runtime-test-support")]
    let providers = fixture_provider::startups(&directory)?;
    #[cfg(not(feature = "runtime-test-support"))]
    let providers = Vec::new();
    let mut runtime =
        CollaborationRuntime::start_with_external_providers(inputs, providers).await?;
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
    println!(
        "{}",
        serde_json::json!({"kind":"boardDebugRuntimeReady","directory":directory,
            "mcpUrl":manifest.mcp.url,"nativeBackend":"absent"})
    );
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
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions fail the test while fixture errors propagate."
    )]
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
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions fail the test while fixture errors propagate."
    )]
    fn launcher_refuses_the_production_port() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        assert!(validate_debug_directory(directory.path(), 8787).is_err());
        assert!(validate_debug_directory(directory.path(), 0).is_ok());
        Ok(())
    }
}
