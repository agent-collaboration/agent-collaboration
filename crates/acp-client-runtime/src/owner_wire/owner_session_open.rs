//! Start SDK routing as an ordered connection effect.

use crate::{
    ExternalProviderRuntimeError, ModelReasoningMetadata, ProviderBehavior,
    ProviderCapabilityReport, ProviderSettingsCatalog, SessionWorkRoots,
    owner_session_stream::{OwnerStreamTarget, ProjectionBuffer},
    provider_item_projection::ProviderItemProjection,
};
use agent_client_protocol::schema::v1::{
    LoadSessionRequest, McpServer, NewSessionRequest, ResumeSessionRequest, SessionId,
};
use agent_client_protocol::{ActiveSession, Agent, ConnectionTo, Dispatch};
use futures_util::FutureExt as _;
use provider_session_machines::session_lifecycle_machine::{LoadAttempt, RestoreVia};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Clone, Debug)]
pub struct OwnerSessionOpenSetup {
    pub cwd: PathBuf,
    pub work_roots: SessionWorkRoots,
    pub scratch_folder: Option<PathBuf>,
}

pub type OwnerSessionOpenReceipt =
    oneshot::Receiver<Result<OwnerOpenedSession, ExternalProviderRuntimeError>>;

pub struct OwnerOpenedSession {
    pub(crate) provider_id: SessionId,
    pub(crate) connection: ConnectionTo<Agent>,
    pub(crate) route: Option<OpenedSessionRoute>,
    pub(crate) capabilities: ProviderCapabilityReport,
    pub(crate) catalog: ProviderSettingsCatalog,
    pub(crate) behavior: Arc<dyn ProviderBehavior>,
    pub(crate) buffer: Arc<ProjectionBuffer>,
    pub(crate) projection: Option<ProviderItemProjection>,
    pub(crate) in_folder: Option<crate::in_folder_allowance::InFolderGrant>,
    pub(crate) router_auto_active: bool,
    #[cfg(any(test, feature = "test-observation"))]
    pub(crate) observed_tool_calls: crate::owner_session_stream::TestToolCallRecords,
}

pub(crate) enum OpenedSessionRoute {
    Created(oneshot::Sender<CreatedStream>),
    Restored {
        sdk: Box<ActiveSession<'static, Agent>>,
        updates: mpsc::UnboundedReceiver<Dispatch>,
    },
}

pub(crate) struct CreatedStream {
    pub(crate) opened: OwnerOpenedSession,
    pub(crate) target: OwnerStreamTarget,
    pub(crate) finished: oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
}

impl OwnerOpenedSession {
    fn new(
        sdk: &ActiveSession<'_, Agent>,
        route: Option<OpenedSessionRoute>,
        config: &OwnerOpenConfiguration,
        in_folder: Option<crate::in_folder_allowance::InFolderGrant>,
        auth_status: session_event_model::ProviderAuthStatus,
    ) -> Self {
        let response = sdk.response();
        let catalog = crate::provider_settings_catalog_codec::catalog_from_session_response(
            &response,
            config.behavior.as_ref(),
            &config.reasoning,
        );
        Self {
            provider_id: sdk.session_id().clone(),
            connection: sdk.connection().clone(),
            route,
            capabilities: config
                .capabilities
                .with_session_response(&response)
                .with_auth_status(auth_status),
            catalog,
            behavior: Arc::clone(&config.behavior),
            buffer: Arc::default(),
            projection: None,
            in_folder,
            router_auto_active: false,
            #[cfg(any(test, feature = "test-observation"))]
            observed_tool_calls: config.observed_tool_calls.clone(),
        }
    }

    #[must_use]
    pub fn provider_id(&self) -> String {
        self.provider_id.to_string()
    }

    pub fn register_in_folder_scope(
        &mut self,
        wire: &crate::owner_wire::OwnerAcpWire,
        run: crate::acp_transport::TransportRun,
    ) {
        if let Some(grant) = self.in_folder.take() {
            wire.register_grant(
                run,
                crate::in_folder_allowance::SessionInFolderGrant {
                    grant,
                    router_auto: self.router_auto_active,
                },
            );
        }
    }
}

#[derive(Clone)]
pub(crate) struct OwnerOpenConfiguration {
    pub(crate) behavior: Arc<dyn ProviderBehavior>,
    pub(crate) capabilities: ProviderCapabilityReport,
    pub(crate) mcp_servers: Vec<McpServer>,
    pub(crate) reasoning: ModelReasoningMetadata,
    pub(crate) auth_status: Arc<tokio::sync::RwLock<session_event_model::ProviderAuthStatus>>,
    #[cfg(any(test, feature = "test-observation"))]
    pub(crate) observed_tool_calls: crate::owner_session_stream::TestToolCallRecords,
}

pub(crate) enum OpenRequest {
    New(OwnerSessionOpenSetup),
    Restore {
        provider_id: String,
        attempt: LoadAttempt,
        updates: mpsc::UnboundedReceiver<Dispatch>,
        via: RestoreVia,
        setup: OwnerSessionOpenSetup,
    },
}

pub(crate) struct OpenFrame {
    pub(crate) request: OpenRequest,
    pub(crate) respond: oneshot::Sender<Result<OwnerOpenedSession, ExternalProviderRuntimeError>>,
}

impl OpenFrame {
    pub(crate) fn start(
        self,
        run: crate::acp_transport::TransportRun,
        connection: &ConnectionTo<Agent>,
        config: &OwnerOpenConfiguration,
        tasks: &TaskTracker,
        shutdown: CancellationToken,
        restore_updates: Arc<crate::owner_restore_updates::RestoreUpdates>,
    ) {
        use crate::provider_acp_error_mapping::{acp_load_session_error, acp_operation_error};

        match self.request {
            OpenRequest::New(setup) => {
                let folders = crate::session_start_folders::session_start_folders(
                    &setup.cwd,
                    setup.scratch_folder.as_deref(),
                    &setup.work_roots,
                    config.behavior.as_ref(),
                    config.capabilities.supports_additional_directories,
                );
                let mut request = NewSessionRequest::new(&setup.cwd)
                    .mcp_servers(config.mcp_servers.clone())
                    .additional_directories(folders.additional_directories);
                request.meta = folders.meta;
                let builder = connection.build_session_from(request).block_task();
                let config = config.clone();
                let start = async move {
                    let mut respond = Some(self.respond);
                    // Keep the SDK route inside its scope and return the
                    // original handshake failure through the same receipt.
                    let result = builder
                        .run_until(async |mut sdk| {
                            let (activate, activation) = oneshot::channel();
                            let auth_status = config.auth_status.read().await.clone();
                            let opened = OwnerOpenedSession::new(
                                &sdk,
                                Some(OpenedSessionRoute::Created(activate)),
                                &config,
                                folders.in_folder,
                                auth_status,
                            );
                            if respond
                                .take()
                                .is_some_and(|reply| reply.send(Ok(opened)).is_ok())
                                && let Ok(CreatedStream {
                                    opened,
                                    target,
                                    finished,
                                }) = activation.await
                            {
                                let _owner_retired =
                                    finished.send(opened.read_sdk_stream(&mut sdk, target).await);
                            }
                            Ok(())
                        })
                        .await;
                    if let (Some(respond), Err(error)) = (respond, result) {
                        let _owner_retired = respond.send(Err(acp_operation_error(error)));
                    }
                };
                let mut start = Box::pin(start);
                // The first poll enqueues the handshake in writer order.
                if start.as_mut().now_or_never().is_none() {
                    tasks.spawn(async move {
                        tokio::select! {
                            () = start => {}
                            () = shutdown.cancelled() => {}
                        }
                    });
                }
            }
            OpenRequest::Restore {
                provider_id,
                attempt,
                updates,
                via,
                setup,
            } => {
                let folders = crate::session_start_folders::session_start_folders(
                    &setup.cwd,
                    setup.scratch_folder.as_deref(),
                    &setup.work_roots,
                    config.behavior.as_ref(),
                    config.capabilities.supports_additional_directories,
                );
                let connection = connection.clone();
                let servers = config.mcp_servers.clone();
                let config = config.clone();
                let start = async move {
                    let requested_session = SessionId::new(provider_id);
                    let sdk = match via {
                        RestoreVia::Load => {
                            let mut request =
                                LoadSessionRequest::new(requested_session.clone(), &setup.cwd)
                                    .mcp_servers(servers)
                                    .additional_directories(folders.additional_directories);
                            request.meta = folders.meta;
                            connection
                                .load_session_from(request)
                                .block_task()
                                .start_session()
                                .await
                                .map(|restored| restored.into_session())
                        }
                        RestoreVia::Resume => {
                            let mut request =
                                ResumeSessionRequest::new(requested_session.clone(), &setup.cwd)
                                    .mcp_servers(servers)
                                    .additional_directories(folders.additional_directories);
                            request.meta = folders.meta;
                            connection
                                .resume_session_from(request)
                                .block_task()
                                .start_session()
                                .await
                                .map(|restored| restored.into_session())
                        }
                    }
                    .map_err(|error| {
                        acp_load_session_error(
                            error,
                            &requested_session.0,
                            config.behavior.as_ref(),
                        )
                    })?;
                    let auth_status = config.auth_status.read().await.clone();
                    let mut opened = OwnerOpenedSession::new(
                        &sdk,
                        None,
                        &config,
                        folders.in_folder,
                        auth_status,
                    );
                    opened.route = Some(OpenedSessionRoute::Restored {
                        sdk: Box::new(sdk),
                        updates,
                    });
                    Ok(opened)
                };
                let mut start = Box::pin(start);
                // The first poll registers the provisional SDK route and
                // enqueues load/resume before the next writer entry.
                if let Some(ready) = start.as_mut().now_or_never() {
                    let _owner_retired = self.respond.send(ready);
                    restore_updates.reply_settled(run);
                } else {
                    tasks.spawn(async move {
                        // Keep this SDK request and its replay tap correlated until
                        // the wire answers. A retired owner's receiver has no successor.
                        let result = tokio::select! {
                            result = start => result,
                            () = shutdown.cancelled() => Err(ExternalProviderRuntimeError::RestoreDisconnected),
                        };
                        if self.respond.send(result).is_err() {
                            tracing::debug!(?run, ?attempt, "settled retired ACP restore");
                        }
                        restore_updates.reply_settled(run);
                    });
                }
            }
        }
    }
}
