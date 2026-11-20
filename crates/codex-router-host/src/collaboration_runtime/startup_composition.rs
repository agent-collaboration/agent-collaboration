//! Bind listeners, prepare service owners and start the Host runtime.
use super::{
    BackendPublication, BoundCollaborationApi, CancellationToken, CollaborationRuntime,
    CollaborationRuntimeInputs, EndpointAvailability, EndpointId, EndpointRef,
    HostCollaborationInputs, JoinSet, NativeRelayListener, NonEmptyText, RemoteControlServerName,
    RouterExecutableRelation, SchemaDigest, SocketAddr, create_private_socket_directory,
    current_observation_timestamp, io, load_installation_peer, load_service_identity,
    new_service_uuid, unix_seconds,
};

impl CollaborationRuntime {
    /// Binds both private listeners before spawning tasks. No native process is started.
    pub async fn start(inputs: CollaborationRuntimeInputs) -> io::Result<Self> {
        Self::start_with_external_providers(inputs, Vec::new()).await
    }

    pub async fn start_with_external_providers(
        inputs: CollaborationRuntimeInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
    ) -> io::Result<Self> {
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            inputs,
            provider_launches,
            std::sync::Arc::new(collaboration_mcp::FixedRouterExecutableRelation(
                RouterExecutableRelation::Match,
            )),
            None,
            None,
            std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
        ))
        .await
    }

    pub(crate) async fn start_for_host_with_router_proxy_endpoint(
        host_inputs: HostCollaborationInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
        router_executable_check: std::sync::Arc<dyn collaboration_mcp::RouterExecutableCheck>,
    ) -> io::Result<Self> {
        let prepared_exposure = crate::tailscale_exposure::PreparedTailscaleExposure::prepare(
            host_inputs.remote_access,
        )
        .await?;
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            host_inputs.collaboration_runtime,
            provider_launches,
            router_executable_check,
            Some(host_inputs.router_proxy_endpoint),
            prepared_exposure,
            std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
        ))
        .await
    }

    #[cfg(feature = "runtime-test-support")]
    pub async fn start_with_test_clock(
        inputs: CollaborationRuntimeInputs,
        worker_clock: std::sync::Arc<dyn router_clock::RuntimeClock>,
    ) -> io::Result<Self> {
        Self::runtime_test_start_with_provider_clock(inputs, Vec::new(), worker_clock).await
    }

    #[cfg(feature = "runtime-test-support")]
    pub async fn runtime_test_start_with_provider_clock(
        inputs: CollaborationRuntimeInputs,
        providers: Vec<crate::ExternalProviderStartup>,
        worker_clock: std::sync::Arc<dyn router_clock::RuntimeClock>,
    ) -> io::Result<Self> {
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            inputs,
            providers,
            std::sync::Arc::new(collaboration_mcp::FixedRouterExecutableRelation(
                RouterExecutableRelation::Match,
            )),
            None,
            None,
            worker_clock,
        ))
        .await
    }

    pub(super) async fn start_with_optional_router_proxy_endpoint(
        inputs: CollaborationRuntimeInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
        router_executable_check: std::sync::Arc<dyn collaboration_mcp::RouterExecutableCheck>,
        router_proxy_endpoint: Option<SocketAddr>,
        prepared_exposure: Option<crate::tailscale_exposure::PreparedTailscaleExposure>,
        worker_clock: std::sync::Arc<dyn router_clock::RuntimeClock>,
    ) -> io::Result<Self> {
        let mut owner_human_id = inputs.owner_human_id.clone();
        let service_id = load_service_identity(&inputs.directory)?;
        let installation_peer = load_installation_peer(&inputs.directory)?;
        let service_epoch = new_service_uuid()?;
        let machine_identity = collaboration_contracts::MachineIdentity::new(
            service_id.clone(),
            inputs
                .remote_control_server_name
                .as_ref()
                .map(RemoteControlServerName::as_str),
        )
        .map_err(io::Error::other)?;
        let native_digest = if let Some(export) = &inputs.native_schema {
            super::schema_bundle_retention::publish_in_use_schema_bundle(
                &inputs.directory,
                export.bundle(),
            )?;
            let encoded = format!(
                "sha256:{}",
                export
                    .bundle()
                    .digest()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            Some(SchemaDigest::try_from(encoded).map_err(io::Error::other)?)
        } else {
            None
        };
        let native_definitions = inputs.native_schema.as_ref().and_then(|export| {
            collaboration_mcp::NativeSchemaDefinitions::from_bundle(export.bundle())
        });
        let journal = async {
            let database = lifecycle_observation::ObservationJournal::open(
                &inputs.directory.join("session-registry.sqlite"),
                new_service_uuid()?,
            )
            .await
            .map_err(io::Error::other)?;
            let journal = std::sync::Arc::new(lifecycle_observation::LifecycleStore::new(database));
            journal
                .prepare(unix_seconds()?)
                .await
                .map_err(io::Error::other)?;
            Ok::<_, io::Error>(journal)
        }
        .await;
        let journal = match journal {
            Ok(journal) => Some(journal),
            Err(_) => {
                tracing::warn!("lifecycle storage unavailable; collaboration remains independent");
                None
            }
        };
        let identity = collaboration_contracts::RouterIdentity::new(
            service_id.clone(),
            service_epoch.clone(),
            Some(installation_peer.clone()),
            machine_identity.clone(),
        )
        .map_err(io::Error::other)?;
        let endpoint_directory = collaboration_runtime::EndpointDirectory::new(service_id.clone());
        let provider_store = match collaboration_runtime::ProviderOperationStore::open(
            &inputs.directory.join("provider-operations.sqlite"),
        )
        .await
        {
            Ok(store) => {
                let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
                Some(store)
            }
            Err(_) => {
                tracing::warn!(
                    "provider operation storage unavailable; external providers remain disabled"
                );
                None
            }
        };
        let proxy_authority = prepared_exposure
            .as_ref()
            .map(|prepared| prepared.authority().clone());
        let collaboration_api =
            BoundCollaborationApi::bind(inputs.mcp_bind, proxy_authority).await?;
        let exposure_backend = collaboration_api.local_address();
        let mcp_url = collaboration_api.url();
        let startup = crate::provider_startup_composition::compose_provider_startup(
            provider_launches,
            provider_store.clone(),
            &service_id,
            &service_epoch,
            &mcp_url,
            inputs
                .directory
                .parent()
                .map(codex_native_integration::router_scratch_root),
            Some(inputs.directory.join("hooks")),
            std::sync::Arc::clone(&worker_clock),
        )
        .await?;
        let provider_face_endpoints = startup.endpoints.clone();
        let provider_retirements = startup.retirements;
        let external_provider_supervisor = startup.supervisor;
        let provider_session_hub = startup.hub;
        let provider_cache_warm = crate::provider_cache_warm::compose_provider_cache_warm(
            startup.cache_warm,
            external_provider_supervisor.as_ref(),
            provider_store.as_ref(),
            worker_clock.clone(),
        );
        let provider_endpoints = startup
            .endpoints
            .iter()
            .map(|description| description.endpoint.clone())
            .collect();
        endpoint_directory.publish_inventory(startup.endpoints)?;
        let provider_conversations = external_provider_supervisor.as_ref().map(|supervisor| {
            supervisor.clone()
                as std::sync::Arc<dyn collaboration_contracts::ProviderConversationBackend>
        });
        let endpoint = EndpointRef {
            service_id: service_id.clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned())
                .map_err(io::Error::other)?,
        };
        let publication = BackendPublication::new(
            endpoint_directory.clone(),
            endpoint.clone(),
            service_epoch.clone(),
            inputs.backend_socket,
        )?;
        let native_backend = collaboration_runtime::NativeControlBackend {
            codex_home: inputs.codex_home.clone(),
            endpoint,
            gate: publication.admission_gate(),
        };
        let unmaterialized_threads =
            std::sync::Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new());
        let open_scratch_scopes =
            std::sync::Arc::new(collaboration_runtime::OpenScratchScopes::new());
        let peer_registry_directory = inputs.peer_registry_directory.clone().map_or_else(
            crate::session_message_route_composition::default_peer_registry_directory,
            Ok,
        )?;
        let claude_background_jobs =
            std::sync::Arc::new(claude_code_peer_messaging::NativeClaudeJobReader::default());
        let claude_code_sessions =
            std::sync::Arc::new(claude_code_peer_messaging::ClaudeCodeSessionRegistry::new(
                peer_registry_directory.clone(),
            ));
        let message_routes =
            crate::session_message_route_composition::compose_session_message_routes(
                crate::session_message_route_composition::SessionMessageRouteInputs {
                    clock: worker_clock.clone(),
                    provider_endpoints,
                    directory: endpoint_directory.clone(),
                    native_backend: native_backend.clone(),
                    unmaterialized_threads: std::sync::Arc::clone(&unmaterialized_threads),
                    provider_supervisor: external_provider_supervisor.clone(),
                    provider_store: provider_store.clone(),
                    peer_registry_directory,
                },
            )?;
        let provider_delivery_route = message_routes.provider_route;
        let session_delivery_router = Some(message_routes.router);
        let codex_recorder = crate::codex_conversation_recording_composition::recording_for_store(
            provider_store.as_ref(),
            &inputs.directory,
            &service_id,
        )?;
        let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(32));
        let application = {
            let native_control =
                collaboration_service::collaboration_application::NativeSessionControl::new(
                    identity.clone(),
                    endpoint_directory.clone(),
                    Some(native_backend.clone()),
                    codex_recorder.clone().map(|recorder| {
                        recorder as std::sync::Arc<dyn collaboration_runtime::CodexPermissionModes>
                    }),
                );
            let codex_inventory = collaboration_service::CodexSessionInventory::new(
                identity.clone(),
                endpoint_directory.clone(),
                Some(native_backend.clone()),
                journal.clone(),
            );
            let provider_inventory = collaboration_service::ProviderSessionInventory::new(
                identity.clone(),
                endpoint_directory.clone(),
                provider_store.clone(),
                provider_session_hub
                    .clone()
                    .map(|hub| hub as std::sync::Arc<dyn collaboration_contracts::SessionEventHub>),
                Some(claude_code_sessions.clone()),
                Some(claude_background_jobs.clone()),
            );
            let journal_operations =
                collaboration_service::collaboration_application::JournalOperations::new(
                    identity.clone(),
                    endpoint_directory.clone(),
                    journal.clone(),
                );
            let sessions = collaboration_service::collaboration_application::SessionOperations::new(
                identity.clone(),
                endpoint_directory.clone(),
                native_control,
                codex_inventory,
                provider_inventory,
                journal_operations,
            );
            let conversations =
                collaboration_service::collaboration_application::ConversationOperations::new(
                    identity.clone(),
                    provider_conversations,
                    provider_store.clone(),
                    codex_recorder.clone(),
                    endpoint_directory.clone(),
                );
            let observation =
                collaboration_service::collaboration_application::ObservationOperations::new(
                    identity.clone(),
                    provider_session_hub.clone().map(|hub| {
                        hub as std::sync::Arc<dyn collaboration_contracts::SessionEventHub>
                    }),
                    endpoint_directory.clone(),
                );
            collaboration_service::CollaborationApplication::new(
                identity.clone(),
                conversations,
                observation,
                sessions,
            )
        };
        let collaboration_api =
            collaboration_api.with_service_socket(&inputs.directory.join("control.sock"))?;
        let native = NativeRelayListener::bind(
            &inputs.directory.join("codex-native.sock"),
            publication.admission_gate(),
        )?
        .with_connection_budget(std::sync::Arc::clone(&permits));
        let stored = std::sync::Arc::new(codex_acp_adapter::NativeStoredSessions::new(
            inputs.codex_home.clone(),
            String::from(service_id.clone()),
        ));
        let mut acp = collaboration_runtime::AcpChannelListener::bind(
            &inputs.directory.join("codex-acp.sock"),
            service_id.clone(),
            publication.admission_gate(),
            stored,
            unmaterialized_threads,
            crate::codex_conversation_recording_composition::adapter_recorder(codex_recorder),
            std::sync::Arc::clone(&open_scratch_scopes)
                as std::sync::Arc<dyn codex_acp_adapter::ScratchScopeRegister>,
        )?
        .with_connection_budget(permits);
        let mut provider_app_servers = Vec::new();
        if !provider_face_endpoints.is_empty() && external_provider_supervisor.is_some() {
            owner_human_id =
                crate::owner_identity_resolution::resolve_face_owner_human_id(owner_human_id).await;
        }
        if let (Some(supervisor), Some(delivery), Some(hub), Some(store)) = (
            &external_provider_supervisor,
            &provider_delivery_route,
            &provider_session_hub,
            &provider_store,
        ) {
            let commands: std::sync::Arc<dyn collaboration_contracts::SessionCommandPort> =
                std::sync::Arc::new(crate::HostSessionCommandPort::new(
                    std::sync::Arc::clone(supervisor),
                    std::sync::Arc::clone(delivery),
                    std::sync::Arc::clone(hub),
                    std::sync::Arc::clone(store),
                    std::sync::Arc::clone(&worker_clock),
                ));
            let events: std::sync::Arc<dyn collaboration_contracts::SessionEventHub> =
                std::sync::Arc::clone(hub) as _;
            let socket_directory = inputs.directory.join("router-sessions");
            if owner_human_id.is_some() {
                create_private_socket_directory(&socket_directory)?;
            }
            for description in provider_face_endpoints {
                if supervisor.runtime_for(&description.endpoint).is_none() {
                    continue;
                }
                let endpoint = message_board::SessionEndpointRef {
                    service_id: message_board::ServiceId::try_from(String::from(
                        description.endpoint.service_id.clone(),
                    ))
                    .map_err(io::Error::other)?,
                    endpoint_id: message_board::EndpointId::try_from(String::from(
                        description.endpoint.endpoint_id.clone(),
                    ))
                    .map_err(io::Error::other)?,
                };
                acp = acp.with_provider_session_backend(
                    endpoint.clone(),
                    std::sync::Arc::clone(&commands),
                    std::sync::Arc::clone(&events),
                );
                let Some(owner_human_id) = owner_human_id.as_ref() else {
                    continue;
                };
                let context = collaboration_runtime::RouterSessionAppServerContext::new(
                    endpoint,
                    message_board::Identity::Human {
                        human_id: owner_human_id.clone(),
                    },
                    std::sync::Arc::clone(&commands),
                    std::sync::Arc::clone(&events),
                    std::sync::Arc::new(
                        crate::provider_model_catalog::SupervisorModelCatalog::new(
                            std::sync::Arc::clone(supervisor),
                        ),
                    ),
                )
                .with_project_trust(std::sync::Arc::new(
                    codex_native_integration::CodexHomeProjectTrust::new(inputs.codex_home.clone()),
                ));
                let socket_path = socket_directory.join(format!(
                    "{}.sock",
                    String::from(description.endpoint.endpoint_id.clone())
                ));
                provider_app_servers.push(
                    collaboration_runtime::RouterSessionAppServerListener::bind(
                        &socket_path,
                        std::sync::Arc::new(context),
                    )?,
                );
            }
        }
        #[cfg(feature = "runtime-test-support")]
        let provider_test_hub = provider_session_hub.as_ref().map(std::sync::Arc::downgrade);
        drop(provider_session_hub);
        let publication = publication.with_acp_listener()?;
        let manifest = collaboration_runtime::ManifestPublication::publish(
            &inputs.directory,
            &collaboration_protocol::ServiceManifest {
                version: collaboration_protocol::SERVICE_MANIFEST_VERSION,
                service_id: machine_identity.service_id().clone(),
                service_epoch: service_epoch.clone(),
                machine_label: machine_identity.machine_label().clone(),
                service_version: NonEmptyText::try_from(env!("CARGO_PKG_VERSION").to_owned())
                    .map_err(io::Error::other)?,
                api: collaboration_protocol::ApiSelector {
                    transport: collaboration_protocol::ApiTransport::StreamableHttpUnix,
                    path: collaboration_protocol::ApiSocketPath::ServiceSocket,
                },
                mcp: collaboration_protocol::McpSelector {
                    transport: collaboration_protocol::McpTransport::StreamableHttp,
                    url: mcp_url,
                },
                native_schema_digest: native_digest.clone(),
                router_proxy_endpoint,
            },
        )?;
        let shutdown = CancellationToken::new();
        let collaboration_api =
            collaboration_api.serve(&collaboration_mcp::CollaborationApiConfig {
                application,
                service_directory: inputs.directory.clone(),
                native_definitions,
                router_executable_check,
                concurrent_requests: collaboration_mcp::DEFAULT_CONCURRENT_REQUESTS,
                shutdown: shutdown.clone(),
            });
        let mut tasks = JoinSet::new();
        let mut provider_retirement_tasks = JoinSet::new();
        tasks.spawn(native.run(shutdown.clone()));
        tasks.spawn(acp.run(shutdown.clone()));
        for app_server in provider_app_servers {
            tasks.spawn(app_server.run(shutdown.clone()));
        }
        for (retirement, mut description) in provider_retirements {
            let directory = endpoint_directory.clone();
            provider_retirement_tasks.spawn(async move {
                retirement.cancelled().await;
                description.availability = EndpointAvailability::Unavailable {
                    observed_at: current_observation_timestamp()?,
                    reason: NonEmptyText::try_from("provider process retired".to_owned())
                        .map_err(io::Error::other)?,
                    fix: Some(
                        NonEmptyText::try_from(
                            "restart the Host to relaunch the provider".to_owned(),
                        )
                        .map_err(io::Error::other)?,
                    ),
                };
                directory.publish(description)
            });
        }
        let maintenance = journal.as_ref().map(|journal| {
            let maintenance_store = std::sync::Arc::clone(journal);
            let maintenance_stop = shutdown.clone();
            let clock = std::sync::Arc::clone(&worker_clock);
            tokio::spawn(async move {
                maintenance_store
                    .run_maintenance(clock, maintenance_stop)
                    .await
            })
        });
        let mut runtime = Self {
            clock: worker_clock,
            installation_peer,
            owner_human_id,
            provider_store,
            #[cfg(feature = "runtime-test-support")]
            provider_test_hub,
            external_provider_supervisor,
            provider_cache_warm,
            provider_delivery_route,
            session_delivery_router,
            provider_retention: None,
            open_scratch_scopes,
            scratch_sweep: None,
            directory: inputs.directory,
            native_digest,
            payload_cache: crate::native_schema_cache::NativeSchemaCache::default(),
            service_id,
            service_epoch,
            publication,
            shutdown,
            tasks,
            provider_retirement_tasks,
            manifest: Some(manifest),
            journal,
            maintenance,
            current_generation: None,
            observer_task: None,
            collaboration_api,
            tailscale_exposure: None,
        };
        if let Some(prepared) = prepared_exposure {
            match prepared
                .start(
                    exposure_backend,
                    &runtime.service_id,
                    &runtime.service_epoch,
                )
                .await
            {
                Ok(exposure) => {
                    eprintln!("Private collaboration MCP: {}", exposure.url());
                    runtime.tailscale_exposure = Some(exposure);
                }
                Err(error) => {
                    let cleanup = runtime.shutdown().await;
                    return Err(match cleanup {
                        Ok(()) => error,
                        Err(cleanup) => io::Error::other(format!(
                            "{error}; collaboration startup cleanup: {cleanup}"
                        )),
                    });
                }
            }
        }
        Ok(runtime)
    }
}
