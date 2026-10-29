//! Bind listeners, prepare service owners and start the Host runtime.
use super::{
    BackendPublication, BoundCollaborationApi, CancellationToken, CollaborationRuntime,
    CollaborationRuntimeInputs, EndpointAvailability, EndpointId, EndpointRef,
    HostCollaborationInputs, JoinSet, NativeRelayListener, NonEmptyText, RemoteControlServerName,
    RouterExecutableRelation, SchemaDigest, SocketAddr, board_composition,
    create_private_socket_directory, current_observation_timestamp, io, load_installation_peer,
    load_service_identity, new_service_uuid, unix_seconds,
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
        let (_relation_sender, relation_receiver) =
            tokio::sync::watch::channel(RouterExecutableRelation::Match);
        Self::start_with_external_providers_and_router_relation(
            inputs,
            provider_launches,
            relation_receiver,
        )
        .await
    }

    pub async fn start_with_external_providers_and_router_relation(
        inputs: CollaborationRuntimeInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
        relation_receiver: tokio::sync::watch::Receiver<RouterExecutableRelation>,
    ) -> io::Result<Self> {
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            inputs,
            provider_launches,
            relation_receiver,
            None,
            None,
        ))
        .await
    }

    pub(crate) async fn start_for_host_with_router_proxy_endpoint(
        host_inputs: HostCollaborationInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
        relation_receiver: tokio::sync::watch::Receiver<RouterExecutableRelation>,
    ) -> io::Result<Self> {
        let prepared_exposure = crate::tailscale_exposure::PreparedTailscaleExposure::prepare(
            host_inputs.remote_access,
        )
        .await?;
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            host_inputs.collaboration_runtime,
            provider_launches,
            relation_receiver,
            Some(host_inputs.router_proxy_endpoint),
            prepared_exposure,
        ))
        .await
    }

    pub(super) async fn start_with_optional_router_proxy_endpoint(
        inputs: CollaborationRuntimeInputs,
        provider_launches: Vec<crate::ExternalProviderStartup>,
        relation_receiver: tokio::sync::watch::Receiver<RouterExecutableRelation>,
        router_proxy_endpoint: Option<SocketAddr>,
        prepared_exposure: Option<crate::tailscale_exposure::PreparedTailscaleExposure>,
    ) -> io::Result<Self> {
        let mut owner_human_id = inputs.owner_human_id.clone();
        let service_id = load_service_identity(&inputs.directory)?;
        let installation_peer = load_installation_peer(&inputs.directory)?;
        if inputs
            .board_session
            .as_ref()
            .is_some_and(|session| session.peer != installation_peer)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "enrollment peer differs from this installation's persisted peer",
            ));
        }
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
            export
                .bundle()
                .publish(&inputs.directory)
                .map_err(io::Error::other)?;
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
            Some(installation_peer),
            machine_identity.clone(),
        )
        .map_err(io::Error::other)?;
        let endpoint_directory = collaboration_runtime::EndpointDirectory::new(service_id.clone());
        let display_names = collaboration_runtime::SessionDisplayNameCache::default();
        let wake_wait_permits = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
        let mut project_claim = None;
        let mut project_replication: std::sync::Arc<
            dyn collaboration_contracts::ProjectReplicationPosition,
        > = std::sync::Arc::new(collaboration_contracts::UnacknowledgedProjectReplication);
        let mut configuration = collaboration_runtime::AutomationConfigurationHandle::default();
        let opened_board = board_composition::open_board_admission(
            &inputs.directory,
            machine_identity.clone(),
            inputs.board_session,
        )
        .await;
        let mut prepared_board = None;
        let project_admission = match opened_board {
            Ok(board) => {
                let admission = board.admission.clone();
                if let Some(claim) = &board.claim {
                    project_claim = Some(claim.clone());
                }
                if let Some(replication) = &board.replication {
                    project_replication = replication.clone();
                }
                prepared_board = Some(board);
                Some(admission)
            }
            Err(error) => {
                tracing::warn!(%error,"board storage unavailable; other collaboration remains independent");
                None
            }
        };
        let mut automation_store = None;
        let mut settings_backend = None;
        match automation_storage::AutomationStore::open(&inputs.directory.join("automation.sqlite"))
            .await
        {
            Ok(store) => {
                let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
                automation_store = Some(std::sync::Arc::clone(&store));
                let handle = collaboration_runtime::AutomationConfigurationHandle::default();
                handle.suspend().await;
                let backend = std::sync::Arc::new(crate::AutomationSettingsFile::new(
                    &inputs.directory,
                    std::sync::Arc::clone(&store),
                    handle.clone(),
                ));
                configuration = handle;
                settings_backend = Some(backend);
            }
            Err(
                error @ (automation_storage::StorageError::MessageUpgradeRequired { .. }
                | automation_storage::StorageError::PreReleaseMailPresent { .. }),
            ) => {
                return Err(io::Error::other(error));
            }
            Err(_) => {
                tracing::warn!("automation storage unavailable; collaboration remains independent")
            }
        }
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
        )
        .await?;
        let provider_face_endpoints = startup.endpoints.clone();
        let provider_retirements = startup.retirements;
        let external_provider_supervisor = startup.supervisor;
        let provider_session_hub = startup.hub;
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
        let approval_broker = collaboration_runtime::ServiceInteractionBroker::load(
            service_id.clone(),
            native_backend.clone(),
            inputs.directory.join("approval-routes.json"),
        )
        .await
        .map_err(io::Error::other)?;
        let unmaterialized_threads =
            std::sync::Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new());
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
                    provider_endpoints,
                    directory: endpoint_directory.clone(),
                    native_backend: native_backend.clone(),
                    approval_broker: approval_broker.clone(),
                    unmaterialized_threads: std::sync::Arc::clone(&unmaterialized_threads),
                    provider_supervisor: external_provider_supervisor.clone(),
                    provider_store: provider_store.clone(),
                    peer_registry_directory,
                },
            )?;
        let session_delivery: std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery> =
            message_routes.router.clone();
        let subscription_presence: std::sync::Arc<
            dyn collaboration_contracts::TargetPresenceProbe,
        > = message_routes.router.clone();
        let scheduled_run_execution: std::sync::Arc<
            dyn collaboration_contracts::ScheduledRunExecution,
        > = message_routes.router.clone();
        let provider_delivery_route = message_routes.provider_route;
        let subscription_clock: std::sync::Arc<dyn collaboration_service::SubscriptionClock> =
            std::sync::Arc::new(collaboration_service::SystemSubscriptionClock);
        let interactions = approval_broker.interaction_store();
        let subscription_delivery = automation_store.as_ref().map(|_| {
            let board_availability = match project_admission.as_ref() {
                Some(admission) => collaboration_service::BoardAvailability::Available(
                    std::sync::Arc::new(tokio::sync::Mutex::new(
                        admission.subscription_window_store(interactions.clone()),
                    )),
                ),
                None => collaboration_service::BoardAvailability::Unavailable,
            };
            collaboration_service::SubscriptionDeliveryService::new(
                collaboration_service::SubscriptionDeliveryServiceProps {
                    board_availability,
                    push_store: interactions.clone(),
                    delivery: std::sync::Arc::clone(&session_delivery),
                    presence: std::sync::Arc::clone(&subscription_presence),
                    machine_identity: machine_identity.clone(),
                    clock: std::sync::Arc::clone(&subscription_clock),
                },
            )
        });
        if let Some(supervisor) = &external_provider_supervisor {
            supervisor
                .install_approval_broker(std::sync::Arc::clone(&approval_broker))
                .await;
        }
        approval_broker
            .install_session_delivery(session_delivery.clone())
            .map_err(io::Error::other)?;
        approval_broker.install_display_names(display_names.clone());
        approval_broker.install_push_context(interactions.clone(), machine_identity.clone());
        let mail_session = prepared_board
            .as_ref()
            .and_then(|board| board.mail_session());
        let mail_delivery = subscription_delivery
            .as_ref()
            .map(|actors| {
                collaboration_service::MailDeliveryRuntime::new(
                    installation_peer,
                    collaboration_service::MailPushDeliveryProps {
                        interactions: interactions.clone(),
                        actors: actors.clone(),
                        presence: subscription_presence.clone(),
                        machine: machine_identity.clone(),
                        clock: subscription_clock.clone(),
                    },
                    mail_session,
                )
                .map(std::sync::Arc::new)
                .map_err(io::Error::other)
            })
            .transpose()?;
        if let (Some(board), Some(mail)) = (&mut prepared_board, &mail_delivery) {
            board.attach_mail(mail);
        }
        if let Some(mail) = &mail_delivery {
            mail.validate_owner(&identity, Some(&interactions))
                .map_err(io::Error::other)?;
            if let Some(store) = &automation_store {
                mail.attach_wake_owner(
                    service_id.clone(),
                    store.clone(),
                    wake_wait_permits.clone(),
                )
                .map_err(io::Error::other)?;
            }
            mail.attach_reader_readiness().map_err(io::Error::other)?;
            mail.attach_project_replication(project_replication.clone())
                .map_err(io::Error::other)?;
        }
        let notification_publisher =
            project_admission
                .as_ref()
                .zip(mail_delivery.as_ref())
                .map(|(admission, mail)| {
                    std::sync::Arc::new(collaboration_service::NotificationPublisher::new(
                        admission.clone(),
                        interactions.clone(),
                        std::sync::Arc::downgrade(mail),
                    ))
                });
        if let (Some(board), Some(publisher)) = (&mut prepared_board, &notification_publisher) {
            board.attach_publisher(publisher);
        }
        if let (Some(service), Some(admission), Some(publisher)) = (
            &subscription_delivery,
            &project_admission,
            &notification_publisher,
        ) {
            service.attach_follow_admission(
                std::sync::Arc::downgrade(admission),
                std::sync::Arc::downgrade(publisher),
            );
        }
        let wake_worker = automation_store.as_ref().map(|store| {
            collaboration_runtime::WakeTimingWorker::new(
                store.clone(),
                session_delivery.clone(),
                configuration.clone(),
                machine_identity.clone(),
                interactions.clone(),
                std::sync::Arc::new(collaboration_service::SubscriptionRuntimeClock::new(
                    subscription_clock.clone(),
                )),
            )
        });
        let push_handoff_worker = automation_store.as_ref().map(|store| {
            collaboration_runtime::PushHandoffWorker::new(store.clone(), interactions.clone())
        });
        let schedule_worker = automation_store.as_ref().map(|store| {
            collaboration_runtime::ScheduleTimingWorker::new(
                store.clone(),
                interactions.clone(),
                scheduled_run_execution.clone(),
                Some(native_backend.clone()),
                configuration.clone(),
                machine_identity.clone(),
            )
        });
        let retention_worker = automation_store.as_ref().map(|store| {
            collaboration_runtime::AutomationRetentionWorker::new(store.clone())
                .with_native_push_store(interactions.clone())
                .with_interaction_broker(approval_broker.clone())
        });
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
                    Some(approval_broker.clone()),
                    display_names.clone(),
                );
            let session_context =
                collaboration_service::collaboration_application::SessionContextReader::new(
                    identity.clone(),
                    native_control.clone(),
                    provider_store.clone(),
                    Some(claude_code_sessions.clone()),
                );
            let codex_inventory = collaboration_service::CodexSessionInventory::new(
                identity.clone(),
                endpoint_directory.clone(),
                Some(native_backend.clone()),
                display_names.clone(),
                journal.clone(),
            );
            let provider_inventory = collaboration_service::ProviderSessionInventory::new(
                identity.clone(),
                endpoint_directory.clone(),
                provider_store.clone(),
                provider_session_hub
                    .clone()
                    .map(|hub| hub as std::sync::Arc<dyn collaboration_contracts::SessionEventHub>),
                display_names.clone(),
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
            let board = collaboration_service::collaboration_application::BoardOperations::new(
                identity.clone(),
                project_admission.clone(),
                project_claim,
                notification_publisher.clone(),
                subscription_delivery.clone(),
                subscription_delivery
                    .as_ref()
                    .map(|_| subscription_presence.clone()),
                subscription_clock.clone(),
                Some(interactions.clone()),
                project_replication,
                session_context,
                project_admission
                    .as_ref()
                    .map(|admission| admission.project_registry()),
            );
            let messages = collaboration_service::collaboration_application::MessageOperations::new(
                identity.clone(),
                Some(interactions.clone()),
                mail_delivery.clone(),
                display_names.clone(),
                project_admission
                    .as_ref()
                    .map(|admission| admission.project_registry()),
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
            let automation =
                collaboration_service::collaboration_application::AutomationOperations::new(
                    identity.clone(),
                    automation_store.clone(),
                    configuration.clone(),
                    settings_backend.as_ref().map(|backend| {
                        backend.clone()
                            as std::sync::Arc<
                                dyn collaboration_runtime::AutomationConfigurationBackend,
                            >
                    }),
                    Some(session_delivery),
                    Some(interactions.clone()),
                    Some(scheduled_run_execution),
                    Some(native_backend.clone()),
                );
            let interactions =
                collaboration_service::collaboration_application::InteractionOperations::new(Some(
                    approval_broker.clone(),
                ));
            let wakes = collaboration_service::collaboration_application::WakeOperations::new(
                &service_id.clone(),
                automation_store.clone().as_ref(),
            )
            .with_wait_capacity(&wake_wait_permits.clone())
            .with_remote(mail_delivery.clone().as_ref());
            collaboration_service::CollaborationApplication::new(
                identity.clone(),
                board,
                conversations,
                messages,
                observation,
                sessions,
                automation,
                wakes,
                interactions,
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
            publication.admission_gate(),
            stored,
            approval_broker.clone(),
            unmaterialized_threads,
            crate::codex_conversation_recording_composition::adapter_recorder(codex_recorder),
        )?
        .with_connection_budget(permits);
        let mut provider_app_servers = Vec::new();
        if !provider_face_endpoints.is_empty() && external_provider_supervisor.is_some() {
            owner_human_id =
                crate::owner_identity_resolution::resolve_face_owner_human_id(owner_human_id).await;
        }
        if external_provider_supervisor.is_some()
            && provider_delivery_route.is_some()
            && provider_session_hub.is_some()
            && provider_store.is_some()
        {
            acp = acp.with_interaction_broker(std::sync::Arc::clone(&approval_broker));
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
                ));
            let events: std::sync::Arc<dyn collaboration_contracts::SessionEventHub> =
                std::sync::Arc::clone(hub) as _;
            let socket_directory = inputs.directory.join("router-sessions");
            if owner_human_id.is_some() {
                create_private_socket_directory(&socket_directory)?;
            }
            for description in provider_face_endpoints {
                let Some(catalog) = supervisor.provider_model_catalog(&description.endpoint) else {
                    continue;
                };
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
                    catalog,
                )
                .with_interaction_broker(std::sync::Arc::clone(&approval_broker))
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
        // Runtime listeners now own these dependencies. Constructor-local hub,
        // router, presence and broker references can retain provider storage
        // transitively and must be gone before exposure-startup cleanup runs.
        #[cfg(test)]
        let startup_test_broker = std::sync::Arc::downgrade(&approval_broker);
        drop(provider_session_hub);
        drop(message_routes.router);
        drop(subscription_presence);
        drop(approval_broker);
        drop(automation_store);
        drop(interactions);
        let publication = publication.with_acp_listener()?;
        if let Some(settings) = settings_backend
            && settings.recover().await.is_err()
        {
            tracing::warn!(
                "automation configuration recovery unavailable; new automation admission is paused"
            );
        }
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
        if let Some(admission) = &project_admission
            && let Err(error) = admission.recover_creating_projects().await
        {
            tracing::warn!(%error, "project creation recovery unavailable; unrelated collaboration remains available");
        }
        let shutdown = CancellationToken::new();
        if let Some(service) = &subscription_delivery
            && let Err(error) = service.start().await
        {
            service.shutdown().await;
            drop(manifest);
            return Err(io::Error::other(error));
        }
        if let Some(publisher) = &notification_publisher {
            publisher.recover().await.map_err(io::Error::other)?;
        }
        // Binding above establishes this runtime owns the listeners before recovery can mutate state.
        let automation_task = wake_worker.map(|worker| tokio::spawn(worker.run(shutdown.clone())));
        let schedule_task =
            schedule_worker.map(|worker| tokio::spawn(worker.run(shutdown.clone())));
        #[cfg(feature = "runtime-test-support")]
        let automation_maintenance_pass = retention_worker
            .as_ref()
            .map(|worker| worker.observe_passes());
        let automation_maintenance =
            retention_worker.map(|worker| tokio::spawn(worker.run(shutdown.clone())));
        let collaboration_api =
            collaboration_api.serve(&collaboration_mcp::CollaborationApiConfig {
                application,
                service_directory: inputs.directory.clone(),
                native_definitions,
                router_executable_relation: relation_receiver,
                concurrent_requests: collaboration_mcp::DEFAULT_CONCURRENT_REQUESTS,
                shutdown: shutdown.clone(),
            });
        let mut tasks = JoinSet::new();
        #[cfg(test)]
        let startup_test_mail = mail_delivery.as_ref().map(std::sync::Arc::downgrade);
        #[cfg(feature = "runtime-test-support")]
        let runtime_test_mail = mail_delivery.as_ref().map(std::sync::Arc::downgrade);
        if let Some(mail) = mail_delivery {
            let mail_shutdown = shutdown.clone();
            tasks.spawn(async move { mail.run(mail_shutdown).await.map_err(io::Error::other) });
        }
        if let Some(worker) = push_handoff_worker {
            let handoff_shutdown = shutdown.clone();
            tasks.spawn(async move {
                worker.run(handoff_shutdown).await;
                Ok(())
            });
        }
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
            tokio::spawn(async move { maintenance_store.run_maintenance(maintenance_stop).await })
        });
        let mut runtime = Self {
            #[cfg(test)]
            startup_test_mail,
            #[cfg(test)]
            startup_test_broker,
            #[cfg(test)]
            startup_test_display_names: display_names.clone(),
            #[cfg(feature = "runtime-test-support")]
            runtime_test_mail,
            notification_publisher,
            installation_peer,
            board_session: None,
            owner_human_id,
            project_admission,
            subscription_delivery,
            provider_store,
            external_provider_supervisor,
            provider_delivery_route,
            provider_retention: None,
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
            automation_task,
            schedule_task,
            automation_maintenance,
            #[cfg(feature = "runtime-test-support")]
            automation_maintenance_pass,
            current_generation: None,
            observer_task: None,
            collaboration_api,
            tailscale_exposure: None,
        };
        runtime.board_session = prepared_board.and_then(board_composition::BoardComposition::start);
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
