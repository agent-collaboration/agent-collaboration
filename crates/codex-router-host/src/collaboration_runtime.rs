//! Host-owned composition of public listeners and lifecycle publication.
use crate::BackendPublication;
use crate::RemoteControlServerName;
use collaboration_protocol::{
    CodexGeneration, EndpointAvailability, EndpointId, EndpointRef, NonEmptyText,
    ObservationTimestamp, ProviderKind, RouterExecutableRelation, SchemaDigest, UuidIdentity,
};
use collaboration_runtime::NativeRelayListener;
use collaboration_runtime::load_installation_peer;
use collaboration_runtime::load_service_identity;
use collaboration_runtime::new_service_uuid;
use std::{io, net::SocketAddr, path::PathBuf};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub struct CollaborationRuntimeInputs {
    /// Enrollment/configuration supplies inputs; this Host owns the session itself.
    pub board_session: Option<board_fleet_sync::BoardSessionInputs>,
    pub owner_human_id: Option<message_board::HumanId>,
    pub directory: PathBuf,
    pub codex_home: PathBuf,
    pub backend_socket: PathBuf,
    pub mcp_bind: std::net::SocketAddr,
    /// An actual executable-bound export, or metadata-only schema admission.
    pub native_schema: Option<std::sync::Arc<codex_native_integration::NativeSchemaExport>>,
    /// Fixture override; normal Host starts read the owner's ~/.claude/sessions.
    pub peer_registry_directory: Option<PathBuf>,
    /// Remote Control machine name observed when this Host started.
    pub remote_control_server_name: Option<RemoteControlServerName>,
}

/// Host-owned startup facts that are not part of the public collaboration runtime inputs.
pub(crate) struct HostCollaborationInputs {
    pub collaboration_runtime: CollaborationRuntimeInputs,
    pub router_proxy_endpoint: SocketAddr,
    pub remote_access: Option<crate::RemoteAccess>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderLaunchBinding {
    pub endpoint_id: EndpointId,
    pub label: NonEmptyText,
    pub provider: ProviderKind,
    pub launch: crate::ExternalProviderLaunch,
}
impl ExternalProviderLaunchBinding {
    pub fn claude(executable: PathBuf, arguments: Vec<String>) -> Result<Self, &'static str> {
        Self::fixed(
            "claude-local",
            "Claude Code",
            ProviderKind::ClaudeCode,
            executable,
            arguments,
        )
    }

    pub fn cursor(executable: PathBuf, arguments: Vec<String>) -> Result<Self, &'static str> {
        Self::fixed(
            "cursor-local",
            "Cursor",
            ProviderKind::Cursor,
            executable,
            arguments,
        )
    }

    fn fixed(
        endpoint_id: &str,
        label: &str,
        provider: ProviderKind,
        executable: PathBuf,
        arguments: Vec<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            endpoint_id: EndpointId::try_from(endpoint_id.to_owned())
                .map_err(|_| "invalid external provider endpoint ID")?,
            label: NonEmptyText::try_from(label.to_owned())?,
            provider,
            launch: crate::ExternalProviderLaunch {
                persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
                executable,
                arguments,
                environment: Vec::new(),
            },
        })
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderKind {
        self.provider
    }

    #[must_use]
    pub const fn command_flags(&self) -> (&'static str, &'static str) {
        match self.provider {
            ProviderKind::ClaudeCode => ("--claude-acp-executable", "--claude-acp-arguments"),
            ProviderKind::Cursor => ("--cursor-acp-executable", "--cursor-acp-arguments"),
        }
    }
}
pub struct BackendSchemaEvidence<'a> {
    pub running_executable: &'a codex_native_integration::ExecutableIdentity,
    pub export: &'a codex_native_integration::NativeSchemaExport,
}
#[derive(Debug, thiserror::Error)]
pub enum CollaborationFailure {
    #[error("local collaboration listener failed: {0}")]
    LocalListener(#[source] io::Error),
    #[error("private Tailscale exposure failed: {0}")]
    TailnetExposure(#[source] io::Error),
}
pub struct CollaborationRuntime {
    #[cfg(feature = "runtime-test-support")]
    runtime_test_mail: Option<std::sync::Weak<collaboration_service::MailDeliveryRuntime>>,
    notification_publisher: Option<std::sync::Arc<collaboration_service::NotificationPublisher>>,
    board_session: Option<board_composition::RunningMachineSession>,
    owner_human_id: Option<message_board::HumanId>,
    project_admission: Option<std::sync::Arc<collaboration_service::ProjectAdmission>>,
    subscription_delivery: Option<collaboration_service::SubscriptionDeliveryService>,
    provider_store:
        Option<std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>>,
    external_provider_supervisor: Option<std::sync::Arc<crate::ExternalProviderSupervisor>>,
    provider_delivery_route: Option<std::sync::Arc<crate::ProviderAcpDeliveryRoute>>,
    provider_retention: Option<tokio::task::JoinHandle<()>>,
    directory: PathBuf,
    /// The native schema bundle the Host started with, published as the manifest's
    /// `nativeSchemaDigest`.
    native_digest: Option<SchemaDigest>,
    payload_cache: crate::native_schema_cache::NativeSchemaCache,
    service_id: UuidIdentity,
    installation_peer: fleet_sync_protocol::PeerId,
    service_epoch: UuidIdentity,
    publication: BackendPublication,
    shutdown: CancellationToken,
    tasks: JoinSet<io::Result<()>>,
    provider_retirement_tasks: JoinSet<io::Result<()>>,
    journal: Option<std::sync::Arc<lifecycle_observation::LifecycleStore>>,
    maintenance: Option<tokio::task::JoinHandle<Result<(), lifecycle_observation::JournalError>>>,
    automation_task: Option<tokio::task::JoinHandle<()>>,
    schedule_task: Option<tokio::task::JoinHandle<()>>,
    automation_maintenance: Option<tokio::task::JoinHandle<()>>,
    current_generation: Option<CodexGeneration>,
    observer_task: Option<tokio::task::JoinHandle<()>>,
    manifest: Option<collaboration_runtime::ManifestPublication>,
    collaboration_api: ServedCollaborationApi,
    tailscale_exposure: Option<crate::tailscale_exposure::TailscaleExposure>,
}
impl CollaborationRuntime {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    pub async fn runtime_test_has_local_cancel_delivery(
        &self,
        cancel: fleet_sync_protocol::ItemId,
    ) -> io::Result<bool> {
        self.runtime_test_mail
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| io::Error::other("test mail runtime is unavailable"))?
            .runtime_test_has_local_cancel_delivery(cancel)
            .await
            .map_err(io::Error::other)
    }

    #[cfg(feature = "runtime-test-support")]
    pub const RUNTIME_TEST_MAIL_FAULT_FILE: &str = "messaging-test-fault.json";

    #[cfg(feature = "runtime-test-support")]
    pub async fn runtime_test_install_mail_fault(
        &self,
        fault: interaction_storage::MailStoreTestFault,
    ) -> io::Result<()> {
        self.runtime_test_mail
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| io::Error::other("test mail runtime is unavailable"))?
            .runtime_test_install_mail_fault(fault)
            .await
            .map_err(io::Error::other)
    }

    #[cfg(feature = "runtime-test-support")]
    #[must_use]
    pub fn runtime_test_project_admission(
        &self,
    ) -> Option<std::sync::Arc<collaboration_service::ProjectAdmission>> {
        self.project_admission.clone()
    }

    #[must_use]
    pub fn owner_human_id(&self) -> Option<&message_board::HumanId> {
        self.owner_human_id.as_ref()
    }

    #[must_use]
    pub fn service_id(&self) -> &UuidIdentity {
        &self.service_id
    }
    #[must_use]
    pub fn installation_peer(&self) -> fleet_sync_protocol::PeerId {
        self.installation_peer
    }
    #[must_use]
    pub fn service_epoch(&self) -> &UuidIdentity {
        &self.service_epoch
    }
}

#[path = "collaboration_runtime/collaboration_api_serving.rs"]
mod collaboration_api_serving;
use collaboration_api_serving::{BoundCollaborationApi, ServedCollaborationApi};
#[path = "collaboration_runtime/board_composition.rs"]
mod board_composition;
#[path = "collaboration_runtime/lifecycle.rs"]
mod lifecycle;
#[path = "collaboration_runtime/native_backend.rs"]
mod native_backend;
#[path = "collaboration_runtime/startup_composition.rs"]
mod startup_composition;

impl Drop for CollaborationRuntime {
    fn drop(&mut self) {
        self.manifest.take();
        self.shutdown.cancel();
        if let Some(publisher) = &self.notification_publisher {
            publisher.cancel();
        }
        if let Some(service) = &self.subscription_delivery {
            service.cancel();
        }
        let _retire = self.publication.admission_gate().retire();
    }
}

fn unix_seconds() -> io::Result<i64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?;
    i64::try_from(elapsed.as_secs()).map_err(io::Error::other)
}

fn create_private_socket_directory(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(path)?;
            if metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0 {
                Ok(())
            } else {
                Err(io::Error::other(
                    "provider socket directory must be private",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

fn current_observation_timestamp() -> io::Result<ObservationTimestamp> {
    ObservationTimestamp::try_from(
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    )
    .map_err(io::Error::other)
}

#[path = "collaboration_runtime/provider_retention.rs"]
mod provider_retention;

#[cfg(test)]
#[path = "collaboration_runtime/startup_failure_tests.rs"]
mod startup_failure_tests;
