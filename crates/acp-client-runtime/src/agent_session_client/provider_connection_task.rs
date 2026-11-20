//! Own the ACP connection task, provider process, and Session admission loop.
#[cfg(any(test, feature = "test-observation"))]
use super::{AtomicU64, ExternalProviderApprovalRefusalWarning};

use super::{
    ActiveApprovalContext, AgentSessionClient, Arc, CancellationToken, Client, Duration,
    ExternalProviderApprovalRefusalReason, ExternalProviderLaunch, ExternalProviderRuntimeError,
    HashMap, InteractionPort, Lines, MAX_ACP_FRAME_BYTES, McpServer, Ordering,
    PermissionDispatchState, ProviderAuthStatusHandler, ProviderCommand,
    ProviderCursorCreatePlanHandler, ProviderCursorQuestionHandler, ProviderCursorTodoHandler,
    ProviderFormElicitationHandler, ProviderFrameObservation, ProviderRequestFallback,
    ProviderRequestSessionGuard, RequestPermissionRequest, SessionEventSink,
    initialize_provider_connection, provider_lifecycle_requests, record_permission_refusal,
    spawn_external_approval_dispatch,
};
impl<P: InteractionPort> AgentSessionClient<P> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Passes the ACP connection inputs to the transport initializer."
    )]
    pub(super) async fn initialize_with_timeout_and_mcp_servers(
        launch: ExternalProviderLaunch,
        model_picker: crate::ProviderModelPicker,
        initialize_timeout: Duration,
        configured_mcp_servers: Vec<McpServer>,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let transport = super::provider_transport::ProviderTransport::spawn(&launch)?;
        Self::initialize_over_transport(
            transport,
            launch.persistence_target,
            model_picker,
            initialize_timeout,
            configured_mcp_servers,
            interaction_port,
            event_sink,
            behavior,
            clock,
        )
        .await
    }

    /// Opens the ACP connection over `transport` and initializes it.
    #[expect(
        clippy::too_many_arguments,
        reason = "The connection's inputs, passed through from the public initializers."
    )]
    pub(super) async fn initialize_over_transport(
        transport: super::provider_transport::ProviderTransport,
        persistence_target: crate::ProviderPersistenceTarget,
        model_picker: crate::ProviderModelPicker,
        initialize_timeout: Duration,
        configured_mcp_servers: Vec<McpServer>,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let initialize_deadline = clock.monotonic_now().after(initialize_timeout);
        let super::provider_transport::ProviderTransport {
            writer,
            reader,
            stderr,
            mut lifetime,
        } = transport;
        let shutdown = CancellationToken::new();
        let sink_closed = CancellationToken::new();
        let client_sink_closed = sink_closed.clone();
        let retirement = CancellationToken::new();
        let frame_observation = Arc::new(ProviderFrameObservation::default());
        let task_frame_observation = Arc::clone(&frame_observation);
        let shutdown_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_shutdown_failed = Arc::clone(&shutdown_failed);
        let task_shutdown = shutdown.clone();
        let connection_retirement = retirement.clone();
        let task_connection_retirement = retirement.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (admission_settled_tx, admission_settled_rx) = tokio::sync::oneshot::channel();
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);
        let (owner_wire, owner_output) = crate::owner_wire::OwnerAcpWire::new(
            std::num::NonZeroUsize::new(32).unwrap_or(std::num::NonZeroUsize::MIN),
            Arc::clone(&behavior),
        );
        let owner_wire = Arc::new(owner_wire);
        let callbacks = Arc::new(crate::owner_callback_routing::OwnerCallbackRouting::new(
            owner_wire.clone(),
        ));
        let permission_callbacks = Arc::clone(&callbacks);
        let request_routes = Arc::clone(&owner_wire);
        let auth_routes = Arc::clone(&owner_wire);
        let restore_routes = Arc::clone(&owner_wire);
        let auth_status = Arc::new(tokio::sync::RwLock::new(
            session_event_model::ProviderAuthStatus::NotReported,
        ));
        let callback_auth_status = Arc::clone(&auth_status);
        let open_auth_status = Arc::clone(&auth_status);
        let callback_event_sink = Arc::clone(&event_sink);
        let todo_state = owner_wire.cursor_plan_items();
        let connection_behavior = Arc::clone(&behavior);
        let permission_behavior = Arc::clone(&behavior);
        let task_event_sink = Arc::clone(&event_sink);
        #[cfg(any(test, feature = "test-observation"))]
        let permission_request_count = Arc::new(AtomicU64::new(0));
        #[cfg(any(test, feature = "test-observation"))]
        let callback_permission_request_count = Arc::clone(&permission_request_count);
        #[cfg(any(test, feature = "test-observation"))]
        let permission_outcome = Arc::new(std::sync::atomic::AtomicU8::new(0));
        #[cfg(any(test, feature = "test-observation"))]
        let callback_permission_outcome = Arc::clone(&permission_outcome);
        let callback_interaction_port = Arc::clone(&interaction_port);

        let approval_contexts = Arc::new(std::sync::Mutex::new(HashMap::<
            String,
            ActiveApprovalContext<P>,
        >::new()));
        let callback_approval_contexts = Arc::clone(&approval_contexts);
        let callback_question_contexts = Arc::clone(&approval_contexts);
        let callback_question_port = Arc::clone(&interaction_port);
        let callback_plan_contexts = Arc::clone(&approval_contexts);
        let callback_plan_port = Arc::clone(&interaction_port);
        let callback_elicitation_contexts = Arc::clone(&approval_contexts);
        let callback_elicitation_port = Arc::clone(&interaction_port);
        let permission_refusal_reasons = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let callback_permission_refusal_reasons = Arc::clone(&permission_refusal_reasons);
        let endpoint_id = Arc::new(tokio::sync::RwLock::new(None::<String>));
        let callback_endpoint_id = Arc::clone(&endpoint_id);
        #[cfg(any(test, feature = "test-observation"))]
        let approval_refusal_warnings = Arc::new(std::sync::Mutex::new(Vec::<
            ExternalProviderApprovalRefusalWarning,
        >::new()));
        #[cfg(any(test, feature = "test-observation"))]
        let callback_approval_refusal_warnings = Arc::clone(&approval_refusal_warnings);
        #[cfg(any(test, feature = "test-observation"))]
        let test_tool_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        #[cfg(any(test, feature = "test-observation"))]
        let observed_tool_calls = Arc::clone(&test_tool_calls);
        let client_clock = Arc::clone(&clock);
        let provider_behavior = Arc::clone(&behavior);
        let task = tokio::spawn(async move {
            use futures_util::StreamExt as _;
            use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

            let stderr_task = stderr.map(|stderr| {
                tokio::spawn(crate::provider_transport_diagnostics::drain_provider_stderr(stderr))
            });
            let callback_claims =
                Arc::new(crate::provider_callback_claims::ProviderCallbackClaims::default());
            let outgoing = crate::provider_transport_diagnostics::ObservedProviderSink::new(
                futures_util::SinkExt::<String>::sink_map_err(
                    FramedWrite::new(writer, LinesCodec::new()),
                    std::io::Error::other,
                ),
                Arc::clone(&callback_claims),
            );
            let incoming_frame_observation = Arc::clone(&task_frame_observation);
            let input_ended = CancellationToken::new();
            let input_error = input_ended.clone();
            let input_eof = input_ended.clone();
            let incoming =
                FramedRead::new(reader, LinesCodec::new_with_max_length(MAX_ACP_FRAME_BYTES))
                    .map(move |line| {
                        if let Ok(frame) = &line {
                            crate::provider_transport_diagnostics::record_incoming_frame(frame);
                        }
                        line.map_err(|error| {
                            input_error.cancel();
                            crate::provider_transport_diagnostics::record_incoming_error();
                            if matches!(
                                error,
                                tokio_util::codec::LinesCodecError::MaxLineLengthExceeded
                            ) {
                                incoming_frame_observation.record_limit_exceeded();
                            }
                            std::io::Error::other(error)
                        })
                    })
                    .filter_map(move |line| {
                        std::future::ready(match line {
                            Ok(frame) => callback_claims.incoming(frame).map(Ok),
                            Err(error) => Some(Err(error)),
                        })
                    })
                    .chain(futures_util::stream::once(async move {
                        input_eof.cancel();
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "provider stdout closed",
                        ))
                    }));

            let connection = Client.builder().name("codex-router-host")
                .with_handler(ProviderRequestSessionGuard::new(request_routes))
                .with_handler(crate::owner_callback_routing::OwnerToolCallHandler::new(Arc::clone(&callbacks)))
                .with_handler(crate::owner_restore_updates::OwnerRestoreUpdateRouter(restore_routes))
                .on_receive_request(
                    async move |request: RequestPermissionRequest, responder, connection| {
                        crate::provider_transport_diagnostics::record_callback_entered(crate::provider_transport_diagnostics::ProviderFrameKind::Permission);
                        #[cfg(any(test, feature = "test-observation"))]
                        callback_permission_request_count.fetch_add(1, Ordering::Relaxed);
                        if !permission_callbacks.allows_session_permission(
                            request.session_id.0.as_ref(),
                            request.tool_call.tool_call_id.0.as_ref(),
                        ) {
                            let result = responder.respond(
                                agent_client_protocol::schema::v1::RequestPermissionResponse::new(
                                    agent_client_protocol::schema::v1::RequestPermissionOutcome::Cancelled,
                                ),
                            );
                            #[cfg(any(test, feature = "test-observation"))]
                            if result.is_ok() {
                                callback_permission_outcome.store(1, Ordering::Relaxed);
                            }
                            return result;
                        }
                        let context = callback_approval_contexts.lock().ok().and_then(|contexts| {
                            let scope = permission_callbacks.current_scope(request.session_id.0.as_ref());
                            contexts.get(request.session_id.0.as_ref()).filter(|context| {
                                scope.is_some_and(|(run, turn)| {
                                    context.incarnation == run.incarnation
                                        && context.generation == run.generation
                                        && context.turn == Some(turn)
                                })
                            }).cloned()
                        });
                        if context.is_some()
                            && callback_interaction_port.managed_refusal().is_none()
                            && let Some(option) = permission_callbacks.in_folder_grant(request.session_id.0.as_ref()).and_then(|grant| {
                                grant.allow_once(&request.tool_call, &request.options, &|action, title| {
                                    permission_behavior.stated_permission_target(action, title)
                                })
                            })
                        {
                            return responder.respond(
                                agent_client_protocol::schema::v1::RequestPermissionResponse::new(
                                    agent_client_protocol::schema::v1::RequestPermissionOutcome::Selected(
                                        agent_client_protocol::schema::v1::SelectedPermissionOutcome::new(option),
                                    ),
                                ),
                            );
                        }
                        let dispatch_state = PermissionDispatchState {
                            interaction_port: Arc::clone(&callback_interaction_port),
                            refusal_reasons: Arc::clone(&callback_permission_refusal_reasons),
                            endpoint_id: Arc::clone(&callback_endpoint_id),
                            persistence_target,
                            #[cfg(any(test, feature = "test-observation"))]
                            refusal_warnings: Arc::clone(&callback_approval_refusal_warnings),
                            #[cfg(any(test, feature = "test-observation"))]
                            permission_outcome: Arc::clone(&callback_permission_outcome),
                        };
                        if context.is_none() {
                            record_permission_refusal(
                                &dispatch_state,
                                request.session_id.0.as_ref(),
                                None,
                                ExternalProviderApprovalRefusalReason::MissingPromptContext,
                            )
                            .await;
                        }
                        spawn_external_approval_dispatch(
                            request,
                            responder,
                            connection,
                            context,
                            dispatch_state,
                        )
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .with_handler(ProviderAuthStatusHandler::new(
                    callback_auth_status,
                    auth_routes,
                    callback_event_sink,
                ))
                .with_handler(ProviderCursorQuestionHandler::new(
                    Arc::clone(&callbacks),
                    callback_question_contexts,
                    callback_question_port,
                ))
                .with_handler(ProviderCursorCreatePlanHandler::new(
                    Arc::clone(&callbacks),
                    Arc::clone(&todo_state),
                    Arc::clone(&task_event_sink),
                    callback_plan_contexts,
                    callback_plan_port,
                    task_shutdown.clone(),
                    sink_closed.clone(),
                ))
                .with_handler(ProviderFormElicitationHandler::new(
                    callback_elicitation_contexts,
                    callback_elicitation_port,
                ))
                .with_handler(ProviderCursorTodoHandler::new(
                    callbacks,
                    todo_state,
                    Arc::clone(&task_event_sink),
                    task_shutdown.clone(),
                    sink_closed.clone(),
                ))
                .with_handler(ProviderRequestFallback)
                .connect_with(
                Lines::new(outgoing, incoming),
                async move |connection| {
                    let initialized = tokio::select! {
                        biased;
                        () = task_shutdown.cancelled() => {
                            return Ok(());
                        }
                        response = router_clock::timeout_at(
                            clock.as_ref(),
                            initialize_deadline,
                            initialize_provider_connection(
                                &connection,
                                model_picker,
                                connection_behavior.initialization_authentication(),
                            ),
                        ) => match response {
                            Ok(response) => response,
                            Err(_) => Err(ExternalProviderRuntimeError::InitializeTimeout),
                        },
                    };
                    let admission = initialized.and_then(super::provider_client_initialization::provider_protocol_admission);
                    let opened = match admission {
                        Ok((admission, capabilities)) => {
                            let reasoning = super::provider_model_discovery::read_connection_models(
                                &connection, connection_behavior.as_ref(), clock.as_ref(), initialize_deadline, &task_shutdown,
                            ).await;
                            let configuration = crate::owner_session_open::OwnerOpenConfiguration {
                                behavior: connection_behavior,
                                mcp_servers: if admission.supports_mcp_http { configured_mcp_servers } else { Vec::new() },
                                capabilities: capabilities.clone(), reasoning,
                                auth_status: open_auth_status,
                                #[cfg(any(test, feature = "test-observation"))]
                                observed_tool_calls,
                            };
                            let _reader_gone = ready_tx.send(Ok((admission, capabilities)));
                            Some(configuration)
                        }
                        Err(error) => { let _reader_gone = ready_tx.send(Err(error)); None }
                    };
                    let _reader_gone = admission_settled_tx.send(());
                    if let Some(configuration) = opened {
                        let mut executors = tokio::task::JoinSet::new();
                        executors.spawn(owner_output.run(connection.clone(), configuration, task_connection_retirement.clone(), input_ended));
                        loop {
                            tokio::select! {
                                () = task_shutdown.cancelled() => break,
                                command = command_rx.recv() => match command {
                                    Some(ProviderCommand { cwd, reply }) => {
                                        let connection = connection.clone();
                                        let stopped = task_shutdown.clone();
                                        executors.spawn(async move {
                                            let result = tokio::select! {
                                                () = stopped.cancelled() => Err(ExternalProviderRuntimeError::TransportFailure),
                                                listed = provider_lifecycle_requests::list_provider_sessions(connection, cwd) => listed,
                                            };
                                            let _reader_gone = reply.send(result);
                                        });
                                    }
                                    None => break,
                                },
                                Some(result) = executors.join_next(), if !executors.is_empty() => {
                                    if result.is_err() { task_shutdown_failed.store(true, Ordering::Relaxed); }
                                }
                            }
                        }
                        task_connection_retirement.cancel();
                        task_shutdown.cancel();

                        while let Some(result) = executors.join_next().await {
                            if result.is_err() { task_shutdown_failed.store(true, Ordering::Relaxed); }
                        }
                    }
                    Ok(())
                },
            );
            tokio::pin!(connection);
            let child_exited = tokio::select! {
                _result = &mut connection => false,
                () = lifetime.exited() => {
                    // The exited child may have flushed its initialize response into stdout.
                    // Let the connection settle admission before retiring it.
                    tokio::select! {
                        _settled = admission_settled_rx => {}
                        _result = &mut connection => {}
                    }
                    true
                },
            };
            connection_retirement.cancel();

            if !child_exited {
                lifetime.end().await;
            }
            if let Some(stderr_task) = stderr_task {
                let _result = stderr_task.await;
            }
        });

        let (admission, base_capabilities) =
            match router_clock::timeout_at(client_clock.as_ref(), initialize_deadline, ready_rx)
                .await
            {
                Ok(Ok(Ok(admission))) => admission,
                Ok(Ok(Err(error))) => {
                    shutdown.cancel();
                    let _result = task.await;
                    return Err(error);
                }
                Ok(Err(_)) => {
                    shutdown.cancel();
                    let _result = task.await;
                    return Err(ExternalProviderRuntimeError::Initialize(
                        "provider connection closed before initialization".to_owned(),
                    ));
                }
                Err(_) => {
                    shutdown.cancel();
                    let _result = task.await;
                    return Err(ExternalProviderRuntimeError::InitializeTimeout);
                }
            };
        Ok(Self {
            admission,
            base_capabilities,
            auth_status,
            clock: client_clock,
            shutdown,
            sink_closed: client_sink_closed,
            retirement,
            task: tokio::sync::Mutex::new(Some(task)),
            shutdown_failed,
            frame_observation,
            commands: command_tx,
            owner_wire,
            provider_behavior,
            #[cfg(any(test, feature = "test-observation"))]
            permission_request_count,
            #[cfg(any(test, feature = "test-observation"))]
            permission_outcome,
            interaction_port,
            _event_sink: event_sink,
            approval_contexts,
            permission_refusal_reasons,
            endpoint_id,
            #[cfg(any(test, feature = "test-observation"))]
            approval_refusal_warnings,
            #[cfg(any(test, feature = "test-observation"))]
            test_tool_calls,
        })
    }
}
