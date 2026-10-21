//! Provider-only proof Host: real collaboration runtime, no native Codex process or home.
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup, HostCoordinationPaths, HostInstance,
};
use codex_router_secret_store::{SecretStore, file_backend::FileSecretStore, model::SecretKey};
use std::{
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

type ExampleResult<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments == ["--help"] {
        println!(
            "provider-only-debug-host RUN_DIRECTORY ROUTER_PORT MCP_PORT CLAUDE_ACP_EXECUTABLE\n\
             RUN_DIRECTORY: existing owner-private direct child of /tmp, with a task-owned\n\
             debug Router snapshot in router/. Start the separate signed debug Router there.\n\
             Ports must be distinct, unused loopback ports (never 8787, 8788 or 43127).\n\
             Reuse the same directory after this Host exits to observe a fresh generation.\n\
             SIGTERM/Ctrl-C shuts down the actual provider; SIGKILL tests abrupt Host death.\n\
             No model input is sent. Select and observe the model through conversation create."
        );
        return Ok(());
    }
    let [run_directory, router_port, mcp_port, adapter] = arguments.as_slice() else {
        return Err("four arguments required; see --help".into());
    };
    if !cfg!(debug_assertions) {
        return Err("debug build and four arguments required; see --help".into());
    }
    let run_directory = PathBuf::from(run_directory);
    validate_private_directory(&run_directory)?;
    if run_directory.canonicalize()?.parent() != Some(Path::new("/tmp").canonicalize()?.as_path()) {
        return Err("run directory must be a direct child of /tmp".into());
    }
    let router_port = parse_port(router_port)?;
    let mcp_port = parse_port(mcp_port)?;
    if router_port == mcp_port {
        return Err("Router and MCP ports must differ".into());
    }
    let adapter = PathBuf::from(adapter);
    if !adapter.is_absolute() || !adapter.is_file() {
        return Err("Claude ACP executable must be an absolute file path".into());
    }
    let router_root = run_directory.join("router");
    validate_private_directory(&router_root)?;
    let token_store = FileSecretStore::open_read_only(router_root.join("secrets"))?;
    let token = token_store.read_secret(&SecretKey::new("local_router_token")?)?;
    if token.expose_secret().is_empty() {
        return Err("debug Router local token is empty".into());
    }
    let mut provider = ExternalProviderLaunchBinding::claude(adapter, Vec::new())?;
    provider.launch.environment = vec![
        (
            "ANTHROPIC_BASE_URL".to_owned(),
            format!("http://127.0.0.1:{router_port}/anthropic"),
        ),
        (
            "ANTHROPIC_AUTH_TOKEN".to_owned(),
            token.expose_secret().to_owned(),
        ),
    ];
    let service_directory = run_directory.join("agent-communication");
    let empty_catalog = run_directory.join("empty-codex-catalog");
    let peer_registry = run_directory.join("peer-registry");
    for directory in [&service_directory, &empty_catalog, &peer_registry] {
        if !directory.exists() {
            std::fs::DirBuilder::new().mode(0o700).create(directory)?;
        }
        validate_private_directory(directory)?;
    }
    let instance = HostInstance::acquire(HostCoordinationPaths::new(
        run_directory.join("operator.sock"),
        run_directory.join("host.lock"),
    ))?;
    instance
        .clear_stale_endpoint_sockets(&service_directory)
        .await?;
    let mut runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            owner_human_id: None,
            directory: service_directory.clone(),
            codex_home: empty_catalog,
            backend_socket: run_directory.join("unused-native.sock"),
            mcp_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, mcp_port)),
            native_schema: None,
            peer_registry_directory: Some(peer_registry),
            remote_control_server_name: None,
        },
        vec![ExternalProviderStartup::Launch(provider)],
    )
    .await?;
    println!(
        "{}",
        serde_json::json!({
            "kind": "providerOnlyHostReady",
            "hostPid": std::process::id(),
            "serviceDirectory": service_directory,
            "serviceId": runtime.service_id(),
            "serviceEpoch": runtime.service_epoch(),
            "routerPort": router_port,
            "mcpPort": mcp_port,
        })
    );
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let stopped = tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _signal = terminate.recv() => Ok(()),
        failure = runtime.listener_failure() => Err(std::io::Error::other(failure)),
    };
    runtime.shutdown().await?;
    stopped?;
    println!("providerOnlyHostStopped");
    Ok(())
}

fn parse_port(value: &str) -> ExampleResult<u16> {
    let port = value.parse::<u16>()?;
    if matches!(port, 0 | 8787 | 8788 | 43127) {
        return Err("select an unprotected nonzero debug port".into());
    }
    Ok(port)
}

fn validate_private_directory(directory: &Path) -> ExampleResult<()> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
        || directory.canonicalize()? != directory
    {
        return Err("runtime directories must be canonical owner-private directories".into());
    }
    Ok(())
}
