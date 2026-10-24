//! Recipient-enabled apparatus Host; no native Codex process or product changes.
//! ROOT PORT [SESSION_JSON] --provider-plan PRIVATE_JSON
//! The plan names exact adapter/Claude paths, a private config directory and an
//! optional API-key environment name. No credential value is accepted on argv.

use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use fleet_sync_session::{Endpoint, PeerCredential};
use serde::Deserialize;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

type Outcome<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderPlan {
    adapter: PathBuf,
    claude_executable: PathBuf,
    configuration_directory: PathBuf,
    api_key_environment: Option<String>,
}

fn private_file(path: &Path, root: &Path) -> Outcome<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != root.metadata()?.uid()
        || path.canonicalize()? != path
        || !path.starts_with(root)
    {
        return Err(
            io::Error::other("plan/session file must be private inside the owned root").into(),
        );
    }
    Ok(())
}

fn session_inputs(path: &Path, root: &Path) -> Outcome<BoardSessionInputs> {
    private_file(path, root)?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let text = |pointer: &str| -> Outcome<&str> {
        value
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| io::Error::other("typed session field absent").into())
    };
    Ok(BoardSessionInputs {
        endpoint: Endpoint::new(text("/endpoint")?)?,
        access: AccessServiceToken::new(text("/access/clientId")?, text("/access/clientSecret")?)?,
        credential: PeerCredential::new(text("/credential")?.to_owned())?,
        peer: serde_json::from_value(value.get("peer").cloned().ok_or("peer absent")?)?,
        receiver: serde_json::from_value(value.get("receiver").cloned().ok_or("receiver absent")?)?,
        relay: serde_json::from_value(value.get("relay").cloned().ok_or("relay absent")?)?,
    })
}

#[tokio::main]
async fn main() -> Outcome<()> {
    if !cfg!(debug_assertions) {
        return Err("only declared debug builds are supported".into());
    }
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let mut items = arguments.iter();
    let root = PathBuf::from(items.next().ok_or("ROOT required")?);
    let port = items.next().ok_or("PORT required")?.parse::<u16>()?;
    if matches!(port, 8787 | 8788 | 43127) || !root.is_absolute() || root.canonicalize()? != root {
        return Err("private canonical root and a nonprotected port required".into());
    }
    let temporary = std::env::temp_dir().canonicalize()?;
    if root == temporary
        || !root.starts_with(&temporary)
        || root.metadata()?.permissions().mode() & 0o077 != 0
    {
        return Err("root must be a private child of the caller's temporary directory".into());
    }
    let mut session = None;
    let mut provider_path = None;
    while let Some(argument) = items.next() {
        if argument == "--provider-plan" {
            provider_path = Some(PathBuf::from(
                items.next().ok_or("provider plan path absent")?,
            ));
        } else if session.is_none() {
            session = Some(PathBuf::from(argument));
        } else {
            return Err("unexpected argument".into());
        }
    }
    let provider_path = provider_path.ok_or("--provider-plan required")?;
    private_file(&provider_path, &root)?;
    let provider: ProviderPlan = serde_json::from_slice(&std::fs::read(provider_path)?)?;
    for executable in [&provider.adapter, &provider.claude_executable] {
        if !executable.is_absolute()
            || executable.canonicalize()? != *executable
            || !executable.is_file()
        {
            return Err("provider executable must be an exact canonical file".into());
        }
    }
    let configuration = &provider.configuration_directory;
    if !configuration.is_absolute()
        || configuration.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some(".claude" | ".codex" | ".codex-router")
            )
        })
        || configuration.canonicalize()? != *configuration
        || !configuration.is_dir()
        || configuration.metadata()?.uid() != root.metadata()?.uid()
        || configuration.metadata()?.permissions().mode() & 0o077 != 0
    {
        return Err("provider configuration is not private/canonical".into());
    }
    let mut binding = ExternalProviderLaunchBinding::claude(provider.adapter, Vec::new())?;
    binding.launch.environment = vec![
        (
            "CLAUDE_CODE_EXECUTABLE".to_owned(),
            provider.claude_executable.to_string_lossy().into_owned(),
        ),
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            configuration.to_string_lossy().into_owned(),
        ),
    ];
    if let Some(name) = provider.api_key_environment {
        if name.is_empty()
            || !name
                .bytes()
                .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit() || value == b'_')
        {
            return Err("API key must be named by environment selector".into());
        }
        let value = std::env::var(name)
            .map_err(|_| io::Error::other("named API-key environment unavailable"))?;
        binding
            .launch
            .environment
            .push(("ANTHROPIC_API_KEY".to_owned(), value));
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?);
    let mut runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            directory: root.clone(),
            codex_home: home.join(".codex"),
            backend_socket: root.join("absent-native.sock"),
            mcp_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            native_schema: None,
            peer_registry_directory: Some(root.join("absent-peer-registry")),
            owner_human_id: None,
            remote_control_server_name: None,
            board_session: session
                .as_ref()
                .map(|path| session_inputs(path, &root))
                .transpose()?,
        },
        vec![ExternalProviderStartup::Launch(binding)],
    )
    .await?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("service.json"))?)?;
    println!(
        "{}",
        serde_json::json!({"kind":"boardDebugRuntimeReady", "directory":root,
        "mcpUrl":manifest.pointer("/mcp/url"), "nativeBackend":"absent", "recipientProvider":"claude-local"})
    );
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let result = tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
        error = runtime.listener_failure() => Err(io::Error::other(error)),
    };
    let stopped = runtime.shutdown().await;
    result?;
    stopped?;
    Ok(())
}
