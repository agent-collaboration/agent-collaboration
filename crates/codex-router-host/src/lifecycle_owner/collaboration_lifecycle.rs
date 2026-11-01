//! Join existing child lifecycle facts to the independent public collaboration runtime.
use crate::{
    AppServerChild, BackendSchemaEvidence, CollaborationRuntime, CollaborationRuntimeInputs,
    HostConfig, HostInstance, RemoteControlServerName,
};
use std::{io, path::PathBuf};

pub(super) struct CollaborationLifecycle {
    owner_human_id: Option<message_board::HumanId>,
    directory: PathBuf,
    router_root: PathBuf,
    codex_home: PathBuf,
    peer_registry_directory: Option<PathBuf>,
    backend_socket: PathBuf,
    mcp_bind: std::net::SocketAddr,
    remote_access: Option<crate::RemoteAccess>,
    router_proxy_endpoint: std::net::SocketAddr,
    external_provider_startups: Vec<crate::ExternalProviderStartup>,
    provider_operation_retention_days: std::num::NonZeroU32,
    remote_control_server_name: Option<RemoteControlServerName>,
    router_executable_relation:
        tokio::sync::watch::Receiver<collaboration_protocol::RouterExecutableRelation>,
    runtime: Option<CollaborationRuntime>,
    published_child: Option<u32>,
    schema_digest: Option<[u8; 32]>,
    machine_login: Option<Box<crate::fleet_machine_login::HostMachineLogin>>,
    assembled_login: Option<codex_router_fleet_state::fleet_login::SavedFleetLogin>,
}
impl CollaborationLifecycle {
    pub(super) async fn start(
        config: &HostConfig,
        instance: &HostInstance,
        child: &AppServerChild,
        remote_control_server_name: Option<RemoteControlServerName>,
        router_executable_relation: tokio::sync::watch::Receiver<
            collaboration_protocol::RouterExecutableRelation,
        >,
        login_startup: crate::fleet_machine_login::MachineLoginStartup,
    ) -> io::Result<Option<Self>> {
        let token_startup = login_startup.resolve().map_err(io::Error::other)?;
        let Some(directory) = config.collaboration_directory() else {
            return Ok(None);
        };
        let preparation_started_at = std::time::Instant::now();
        let router_secret_root = config.coordination_paths().router_secret_root();
        let router_root = config
            .coordination_paths()
            .router_root()
            .ok_or_else(|| io::Error::other("fleet enrollment requires a Router root"))?
            .to_owned();
        let machine_login = match token_startup {
            None => None,
            Some(inputs) => {
                let secret_root = router_secret_root
                    .as_ref()
                    .ok_or_else(|| io::Error::other("machine login requires a Router secret root"))?
                    .clone();
                let edge_root = router_root.clone();
                let edge =
                    tokio::task::spawn_blocking(move || load_machine_edge_reference(&edge_root))
                        .await
                        .map_err(io::Error::other)??;
                Some(Box::new(
                    Box::pin(crate::fleet_machine_login::start_machine_login(
                        &secret_root,
                        directory,
                        edge,
                        inputs,
                    ))
                    .await?,
                ))
            }
        };
        let external_provider_startups =
            crate::claude_provider_launch_environment::configure_claude_provider_launches(
                config.external_provider_startups().to_vec(),
                config.router_endpoint(),
                router_secret_root.as_deref(),
            )
            .await?;
        crate::record_debug_readiness_timing(
            "providerEnvironmentConfigured",
            preparation_started_at,
        );
        let mut owner = Self {
            owner_human_id: config.owner_human_id().cloned(),
            directory: directory.to_owned(),
            router_root,
            codex_home: config
                .collaboration_codex_home()
                .ok_or_else(|| io::Error::other("collaboration requires explicit Codex home"))?
                .to_owned(),
            peer_registry_directory: config
                .collaboration_peer_registry_directory()
                .map(PathBuf::from),
            backend_socket: config.app_server_socket().to_owned(),
            mcp_bind: config.mcp_bind(),
            remote_access: config.remote_access(),
            router_proxy_endpoint: config.router_endpoint(),
            external_provider_startups,
            provider_operation_retention_days: config.provider_operation_retention_days(),
            remote_control_server_name,
            router_executable_relation,
            runtime: None,
            published_child: None,
            schema_digest: None,
            machine_login,
            assembled_login: None,
        };
        if let Err(error) = owner.synchronize(instance, Some(child)).await {
            let cleanup = owner.shutdown().await;
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => io::Error::other(format!("{error}; startup cleanup: {cleanup}")),
            });
        }
        crate::record_debug_readiness_timing("collaborationSynchronized", preparation_started_at);
        Ok(Some(owner))
    }
    pub(super) async fn synchronize(
        &mut self,
        instance: &HostInstance,
        child: Option<&AppServerChild>,
    ) -> io::Result<()> {
        let Some(child) = child else {
            if self.published_child.take().is_some()
                && let Some(runtime) = &mut self.runtime
            {
                runtime
                    .backend_unavailable(
                        timestamp()?,
                        "Native backend transitioning"
                            .to_owned()
                            .try_into()
                            .map_err(io::Error::other)?,
                    )
                    .await?;
            }
            return Ok(());
        };
        let learned = self
            .machine_login
            .as_ref()
            .and_then(|login| login.route().saved().cloned());
        let route_changed = self.assembled_login.as_ref() != learned.as_deref();
        if self.runtime.is_some() && route_changed {
            self.shutdown_runtime().await?;
        }
        if self.published_child == Some(child.process.process_id()) {
            if let (Some(handle), Some(saved)) = (self.machine_login_handle(), learned) {
                handle.route_adopted(saved);
            }
            return Ok(());
        }
        let export = child.schema_export();
        let digest = export.as_ref().map(|export| *export.bundle().digest());
        if self.runtime.is_some() && digest != self.schema_digest {
            // The manifest's native schema digest and the tool schemas bound to it are fixed for
            // a runtime; a changed native schema starts a new one.
            self.shutdown_runtime().await?;
        }
        if self.runtime.is_none() {
            let inputs_started_at = std::time::Instant::now();
            let board_session =
                load_board_session_inputs(&self.router_root, self.machine_login.as_deref()).await?;
            use std::os::unix::fs::DirBuilderExt;
            match std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&self.directory)
            {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            instance
                .clear_stale_endpoint_sockets(&self.directory)
                .await?;
            crate::record_debug_readiness_timing("collaborationInputsReady", inputs_started_at);
            let composition_started_at = std::time::Instant::now();
            crate::record_debug_readiness_timing(
                "collaborationRuntimeBegin",
                composition_started_at,
            );
            let mut runtime = CollaborationRuntime::start_for_host_with_router_proxy_endpoint(
                crate::HostCollaborationInputs {
                    collaboration_runtime: CollaborationRuntimeInputs {
                        board_session,
                        directory: self.directory.clone(),
                        codex_home: self.codex_home.clone(),
                        backend_socket: self.backend_socket.clone(),
                        mcp_bind: self.mcp_bind,
                        native_schema: export.clone(),
                        peer_registry_directory: self.peer_registry_directory.clone(),
                        remote_control_server_name: self.remote_control_server_name.clone(),
                        owner_human_id: self.owner_human_id.clone(),
                    },
                    router_proxy_endpoint: self.router_proxy_endpoint,
                    remote_access: self.remote_access,
                    machine_login: self.machine_login.as_ref().map(|login| login.binding()),
                },
                self.external_provider_startups.clone(),
                self.router_executable_relation.clone(),
            )
            .await?;
            crate::record_debug_readiness_timing(
                "collaborationRuntimeReady",
                composition_started_at,
            );
            let retention_started_at = std::time::Instant::now();
            if let Err(error) = runtime
                .configure_provider_operation_retention(self.provider_operation_retention_days)
                .await
            {
                let cleanup = runtime.shutdown().await;
                return Err(match cleanup {
                    Ok(()) => error,
                    Err(cleanup) => {
                        io::Error::other(format!("{error}; startup cleanup: {cleanup}"))
                    }
                });
            }
            crate::record_debug_readiness_timing(
                "providerRetentionConfigured",
                retention_started_at,
            );
            self.runtime = Some(runtime);
            self.schema_digest = digest;
        }
        let runtime = self
            .runtime
            .as_mut()
            .ok_or_else(|| io::Error::other("collaboration runtime missing"))?;
        runtime
            .backend_ready(
                timestamp()?,
                export.as_ref().map(|export| BackendSchemaEvidence {
                    running_executable: child.identity(),
                    export,
                }),
            )
            .await?;
        self.published_child = Some(child.process.process_id());
        self.assembled_login = learned.as_ref().map(|saved| (**saved).clone());
        if let (Some(handle), Some(saved)) = (self.machine_login_handle(), learned) {
            handle.route_adopted(saved);
        }
        Ok(())
    }
    pub(super) async fn listener_failure(&mut self) -> crate::CollaborationFailure {
        match &mut self.runtime {
            Some(runtime) => runtime.listener_failure().await,
            None => std::future::pending().await,
        }
    }
    pub(super) fn mcp_listen_address(&self) -> Option<std::net::SocketAddr> {
        self.runtime
            .as_ref()
            .map(CollaborationRuntime::mcp_listen_address)
    }

    pub(super) async fn shutdown(&mut self) -> io::Result<()> {
        let runtime = self.shutdown_runtime().await;
        let login = match self.machine_login.take() {
            Some(login) => (*login).shutdown().await,
            None => Ok(()),
        };
        runtime.and(login)
    }

    pub(super) fn machine_login_handle(
        &self,
    ) -> Option<crate::fleet_machine_login::MachineLoginHandle> {
        self.machine_login
            .as_ref()
            .map(|login| login.binding().handle())
    }

    pub(super) fn route_changes(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<crate::fleet_machine_login::MachineLoginRoute>> {
        self.machine_login_handle()
            .map(|handle| handle.route_changes())
    }

    pub(super) fn route_unavailable(&self) {
        if let Some(handle) = self.machine_login_handle()
            && let Some(saved) = handle.route().saved()
        {
            handle.route_unavailable(saved.clone());
        }
    }

    pub(super) async fn shutdown_runtime(&mut self) -> io::Result<()> {
        if let Some(handle) = self.machine_login_handle() {
            handle.suspend_route().await;
            handle.attach_connection(None);
        }
        self.published_child = None;
        self.assembled_login = None;
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown().await?;
        }
        Ok(())
    }
}

async fn load_board_session_inputs(
    router_root: &std::path::Path,
    machine_login: Option<&crate::fleet_machine_login::HostMachineLogin>,
) -> io::Result<Option<board_fleet_sync::BoardSessionInputs>> {
    match machine_login {
        Some(login) => login.session_inputs(),
        None => {
            let router_root = router_root.to_owned();
            tokio::task::spawn_blocking(move || {
                crate::fleet_enrollment_file::load_router_fleet_enrollment(&router_root)
            })
            .await
            .map_err(io::Error::other)?
        }
    }
}

fn load_machine_edge_reference(
    root: &std::path::Path,
) -> io::Result<crate::fleet_machine_login::MachineEdgeAccess> {
    use crate::fleet_machine_login::MachineEdgeAccess;
    use codex_router_secret_store::model::SecretKey;
    let invalid = || io::Error::other("invalid configured Fleet Access reference");
    Ok(
        match crate::fleet_enrollment_file::load_router_fleet_access(root)? {
            None => MachineEdgeAccess::Unconfigured,
            Some(crate::FleetAccess::Cloudflare(reference)) => MachineEdgeAccess::Cloudflare {
                client_id: reference.client_id,
                secret_key: SecretKey::new(reference.client_secret_key).map_err(|_| invalid())?,
            },
            Some(crate::FleetAccess::SelfHosted(
                crate::SelfHostedAccessReference::SelfHosted { assertion_key },
            )) => MachineEdgeAccess::SelfHosted {
                assertion_key: SecretKey::new(assertion_key).map_err(|_| invalid())?,
            },
        },
    )
}

pub(super) async fn wait_route_change(
    route: &mut Option<tokio::sync::watch::Receiver<crate::fleet_machine_login::MachineLoginRoute>>,
) {
    match route {
        Some(route) => {
            if route.changed().await.is_err() {
                std::future::pending().await
            } else {
                route.borrow_and_update();
            }
        }
        None => std::future::pending().await,
    }
}
pub(super) async fn wait_failure(
    owner: &mut Option<CollaborationLifecycle>,
) -> crate::CollaborationFailure {
    match owner {
        Some(owner) => owner.listener_failure().await,
        None => std::future::pending().await,
    }
}
fn timestamp() -> io::Result<collaboration_protocol::ObservationTimestamp> {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .try_into()
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::load_board_session_inputs;
    use codex_router_fleet_state::test_support::MemoryFleetLoginRepository;
    use std::{os::unix::fs::PermissionsExt, sync::Arc};

    // Oracle: explicit modes, Lead01a197a4. A row-based fallback would read
    // this real private legacy file and fail instead of staying logged out.
    // Access is the positive read control. No pre-fix red observed.
    #[tokio::test]
    async fn fleet_machine_login_empty_tokens_never_read_the_access_enrollment() {
        // Arrange
        let root = tempfile::tempdir().unwrap();
        let path = crate::router_fleet_enrollment_file(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy_bytes = b"invalid legacy enrollment inputs";
        std::fs::write(&path, legacy_bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let login = crate::fleet_machine_login::empty_machine_token_login_fixture(Arc::new(
            MemoryFleetLoginRepository::new(),
        ));

        // Act
        let token_inputs = load_board_session_inputs(root.path(), Some(&login)).await;
        let access_inputs = load_board_session_inputs(root.path(), None).await;

        // Assert: no pump can be prepared in empty token mode, while Access
        // still follows its existing file reader and reports invalid inputs.
        assert!(token_inputs.unwrap().is_none());
        assert!(
            access_inputs
                .err()
                .unwrap()
                .to_string()
                .contains("invalid fleet enrollment inputs")
        );
        assert_eq!(std::fs::read(&path).unwrap(), legacy_bytes);
        login.shutdown().await.unwrap();
    }
}
