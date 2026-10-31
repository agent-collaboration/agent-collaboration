//! An independently stoppable Host, using the central owner's isolated enrollment.
use super::{
    central_owner::emit,
    rig_inputs::{RigInputs, RigResult, require_private_directory},
};
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup, HostCoordinationPaths, HostInstance, load_fleet_enrollment,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::DirBuilderExt,
};

pub async fn run(inputs: &RigInputs, label: &str) -> RigResult {
    let directory = inputs.directory.join(label);
    require_private_directory(&directory)?;
    let instance = HostInstance::acquire(HostCoordinationPaths::new(
        directory.join("operator.sock"),
        directory.join("host.lock"),
    ))?;
    instance.clear_stale_endpoint_sockets(&directory).await?;
    let board_session = load_fleet_enrollment(&directory.join("fixture-session.json"), &directory)?;
    let registry = directory.join("peers");
    if !registry.exists() {
        std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
    }
    require_private_directory(&registry)?;
    let home = std::env::var_os("HOME").ok_or("normal Codex home unavailable")?;
    let providers = match &inputs.claude_acp {
        None => Vec::new(),
        Some(adapter) => vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::claude(adapter.clone(), Vec::new())?,
        )],
    };
    let mut runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: Some(board_session),
            owner_human_id: None,
            directory: directory.clone(),
            codex_home: std::path::PathBuf::from(home).join(".codex"),
            backend_socket: directory.join("absent-native.sock"),
            mcp_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            native_schema: None,
            peer_registry_directory: Some(registry),
            remote_control_server_name: None,
        },
        providers,
    )
    .await?;
    let outcome: RigResult = async {
        emit(
            &serde_json::json!({"kind":"centralAutomationsHostReady","machine":label,
            "pid":std::process::id(), "serviceId":runtime.service_id(),
            "epoch":runtime.service_epoch(), "directory":directory,
            "native":"unavailable", "modelInputSent":false}),
        )?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal.map_err(Into::into),
            _signal = terminate.recv() => Ok(()),
            failure = runtime.listener_failure() => Err(failure.into()),
        }
    }
    .await;
    runtime.shutdown().await?;
    outcome?;
    emit(&serde_json::json!({"kind":"centralAutomationsHostStopped","machine":label}))
}
