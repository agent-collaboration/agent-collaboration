use super::{
    Arc, ConversationBindingIdentity, ConversationCancelRequest, ConversationCloseRequest,
    ConversationCreateRequest, ConversationLoadRequest, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationReconcileRequest,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationSubmission,
    ConversationOperationWaitOutput, ConversationOperationWaitRequest,
    ConversationOperationWaitResult, ConversationOutputUnavailableReason,
    ConversationPromptRequest, ConversationResumeRequest, Duration, EndpointRef,
    ExternalProviderRuntimeError, ExternalProviderSupervisor, OperationId, PathBuf,
    PreparedOperation, ProviderBindingIdentity, ProviderConversationBackend,
    ProviderConversationFuture, ProviderOperationCompletion, ProviderOperationEffect,
    ProviderOperationKind, ProviderOperationStage, ProviderPromptContentsRequest,
    ProviderReconciliationState, ProviderSessionInspectFuture, ProviderSessionInspectRequest,
    ProviderSettingsAcceptRequest, ProviderSettingsFuture, ProviderSettingsSetRequest, SessionId,
    SessionRef, admitted_submission, effective_settings, existing_submission, failure,
    invalid_setting_failure, now_ms, provider_applied_setting, provider_failed_setting,
    provider_session_inspection, provider_session_lifecycle, provider_session_record,
    provider_settings_control, retain_operation, runtime_failure, snapshot_failure,
    snapshot_from_record, unavailable_failure,
};

impl ProviderConversationBackend for ExternalProviderSupervisor {
    fn resume(
        &self,
        request: ConversationResumeRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        provider_session_lifecycle::resume(self, request)
    }

    fn close(
        &self,
        request: ConversationCloseRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        provider_session_lifecycle::close(self, request)
    }

    fn inspect_session(
        &self,
        request: ProviderSessionInspectRequest,
    ) -> ProviderSessionInspectFuture<'_> {
        Box::pin(provider_session_inspection::inspect(self, request))
    }

    fn settings_set(&self, request: ProviderSettingsSetRequest) -> ProviderSettingsFuture<'_> {
        Box::pin(provider_settings_control::set(self, request))
    }

    fn settings_accept(
        &self,
        request: ProviderSettingsAcceptRequest,
    ) -> ProviderSettingsFuture<'_> {
        Box::pin(provider_settings_control::accept(self, request))
    }

    fn binding(&self, endpoint: &EndpointRef) -> Option<ProviderBindingIdentity> {
        self.inner
            .bindings
            .get(endpoint)
            .map(|binding| binding.identity.clone())
    }

    fn lookup_existing(
        &self,
        operation_id: OperationId,
    ) -> ProviderConversationFuture<'_, Option<ConversationOperationSnapshot>> {
        Box::pin(async move {
            self.inner
                .store
                .lock()
                .await
                .inspect(&operation_id)
                .await
                .map_err(|_| unavailable_failure(operation_id.clone(), None))?
                .map(snapshot_from_record)
                .transpose()
                .map_err(|message| snapshot_failure(operation_id, message))
        })
    }

    fn create(
        &self,
        request: ConversationCreateRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        let operation_id = request.operation_id.clone();
        let backend = self.clone();
        retain_operation(operation_id, None, async move {
            let (binding, runtime) =
                backend.runtime_binding(&request.endpoint, &request.operation_id, None)?;
            // Owner, 2026-11-15: "default shodu lbe -> error force agent to ask
            // -- mode auto". A Claude or Cursor start names its mode (D1 R1).
            if request
                .settings
                .as_ref()
                .and_then(|settings| settings.mode.as_deref())
                .is_none()
            {
                return Err(failure(
                    ConversationOperationFailureKind::InvalidSetting,
                    ConversationOperationFailureStage::Validation,
                    ProviderOperationEffect::None,
                    "a mode is required: use --mode auto, or a mode this agent offers",
                    request.operation_id.clone(),
                    None,
                ));
            }
            if let Some(mode) = request
                .settings
                .as_ref()
                .and_then(|settings| settings.mode.as_deref())
                && super::provider_full_access_guard::is_full_access_mode(
                    binding.runtime.provider,
                    mode,
                )
            {
                return Err(failure(
                    ConversationOperationFailureKind::AuthenticationRequired,
                    ConversationOperationFailureStage::Validation,
                    ProviderOperationEffect::None,
                    super::provider_full_access_guard::FULL_ACCESS_REFUSAL,
                    request.operation_id.clone(),
                    None,
                ));
            }
            // Claude in auto never starts without its work-roots hook (D1 R18).
            if request
                .settings
                .as_ref()
                .and_then(|settings| settings.mode.as_deref())
                == Some("auto")
                && let crate::provider_work_roots::WorkRootsHook::Unavailable(reason) =
                    backend.work_roots_for(&binding.endpoint).hook
            {
                return Err(failure(
                    ConversationOperationFailureKind::Unavailable,
                    ConversationOperationFailureStage::Validation,
                    ProviderOperationEffect::None,
                    &format!(
                        "this agent can't start in auto: the Router's work-roots hook isn't available ({reason}); start it in a named mode, or fix the Host and restart it"
                    ),
                    request.operation_id.clone(),
                    None,
                ));
            }
            let prepared = backend
                .prepare_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationCreate,
                    binding.clone(),
                    None,
                )
                .await?;
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { snapshot, live } => {
                    let operation_id = request.operation_id.clone();
                    let requested_policy = request.requested_policy;
                    let endpoint = binding.endpoint;
                    let working_directory = request.working_directory;
                    let created_by = request.created_by;
                    let approver = request.approver;
                    let requested_settings = request.settings.unwrap_or_default();
                    // A session started in auto works in the Router's work
                    // roots (D1 R18); a named mode keeps only its own folders.
                    let auto = requested_settings.mode.as_deref() == Some("auto");
                    let configured_roots = if auto {
                        backend.work_roots_for(&endpoint)
                    } else {
                        crate::provider_work_roots::ProviderWorkRoots::default()
                    };
                    let named_roots: Vec<PathBuf> = if auto {
                        request
                            .work_roots
                            .iter()
                            .map(|root| PathBuf::from(String::from(root.clone())))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let named_work_roots = request.work_roots;
                    let scratch_root = backend.inner.scratch_root.clone();
                    let completion_inner = Arc::clone(&backend.inner);
                    backend.spawn_operation(operation_id.clone(), live, async move {
                        let scratch_folder = match super::provider_session_scratch::prepare_provider_session_scratch(scratch_root.as_deref()).await {
                            Ok(scratch_folder) => scratch_folder,
                            Err(message) => {
                                return ProviderOperationCompletion::Failure(failure(
                                    ConversationOperationFailureKind::Unavailable,
                                    ConversationOperationFailureStage::Dispatch,
                                    ProviderOperationEffect::None,
                                    message,
                                    operation_id,
                                    None,
                                ));
                            }
                        };
                        let cwd = PathBuf::from(String::from(working_directory.clone()));
                        let work_roots = crate::provider_session_setup::session_work_roots(&configured_roots, &cwd, scratch_folder.as_deref(), &named_roots, auto);
                        let resume = crate::provider_operation_settlement::created_session_resume_inputs(requested_settings.mode.clone(), named_work_roots, scratch_folder.as_deref());
                        match runtime
                            .create_session_with_work_roots(
                                cwd,
                                scratch_folder,
                                acp_client_runtime::RequestedProviderSettings {
                                    mode: requested_settings.mode,
                                    model: requested_settings.model,
                                    effort: requested_settings.effort,
                                    // Owner, 2026-11-15: "--fast off is default in ckursor ...
                                    // unless user provices --fast auto" (D1 R7).
                                    fast: if requested_settings.fast == Some(collaboration_protocol::FastSetting::Auto) {
                                        acp_client_runtime::FastRequest::Leave
                                    } else {
                                        acp_client_runtime::FastRequest::OffIfOffered
                                    },
                                },
                                work_roots,
                            )
                            .await
                        {
                            Ok(created) => {
                                let catalog = runtime.settings_catalog(&created.provider_session_id).await;
                                match SessionId::try_from(created.provider_session_id) {
                                    Ok(session_id) => {
                                        let target = SessionRef {
                                            endpoint,
                                            session_id,
                                        };
                                        if let Some(catalog) = catalog {
                                            completion_inner.record_settings_catalog(target.clone(), catalog).await;
                                        }
                                        let mut observed = effective_settings(requested_policy.clone());
                                        observed.mode = created.effective_settings.mode;
                                        observed.model = created.effective_settings.model;
                                        observed.effort = created.effective_settings.effort;
                                        observed.fast = created.effective_settings.fast.map(crate::provider_operation_settlement::provider_fast_state);
                                        observed.requested_mode = created.permission_mode.requested;
                                        observed.mode_reason = created.permission_mode.reason.map(crate::provider_operation_settlement::provider_mode_reason);
                                        runtime.session_setup().record_requested_mode(&String::from(target.session_id.clone()), crate::provider_session_setup::RequestedModeRecord { requested_mode: observed.requested_mode.clone(), mode_reason: observed.mode_reason });
                                        ProviderOperationCompletion::Success {
                                            settlement: ConversationOperationSettlement::Created {
                                                target: target.clone(),
                                                effective_settings: observed,
                                            },
                                            target: Some(target.clone()),
                                            session_record: Some(Box::new(
                                                provider_session_record(
                                                    target,
                                                    working_directory,
                                                    requested_policy,
                                                    created_by,
                                                    approver,
                                                    resume,
                                                ),
                                            )),
                                        }
                                    }
                                    Err(_) => ProviderOperationCompletion::Failure(failure(
                                        ConversationOperationFailureKind::ProtocolViolation,
                                        ConversationOperationFailureStage::Settlement,
                                        ProviderOperationEffect::Applied,
                                        "provider returned an invalid conversation identity",
                                        operation_id,
                                        None,
                                    )),
                                }
                            }
                            Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
                                provider_session_id,
                                applied,
                                failed,
                                not_applied,
                                permission_mode,
                            }) => {
                                let catalog = runtime.settings_catalog(&provider_session_id).await;
                                match SessionId::try_from(provider_session_id) {
                                Ok(session_id) => {
                                    let target = SessionRef { endpoint, session_id };
                                    if let Some(catalog) = catalog {
                                        completion_inner.record_settings_catalog(target.clone(), catalog).await;
                                    }
                                    // Show keeps the requested mode and reason of a partial start too.
                                    runtime.session_setup().record_created_mode(&String::from(target.session_id.clone()), &permission_mode);
                                    ProviderOperationCompletion::Success {
                                        settlement: ConversationOperationSettlement::CreatedWithoutSettings {
                                            target: target.clone(),
                                            applied: applied.into_iter().map(provider_applied_setting).collect(),
                                            failed: vec![provider_failed_setting(failed)],
                                            not_applied: not_applied.into_iter().map(crate::provider_operation_settlement::provider_not_applied_setting).collect(),
                                        },
                                        target: Some(target.clone()),
                                        session_record: Some(Box::new(provider_session_record(
                                            target,
                                            working_directory,
                                            requested_policy,
                                            created_by,
                                            approver,
                                            resume,
                                        ))),
                                    }
                                }
                                Err(_) => ProviderOperationCompletion::Failure(failure(
                                    ConversationOperationFailureKind::ProtocolViolation,
                                    ConversationOperationFailureStage::Settlement,
                                    ProviderOperationEffect::Applied,
                                    "provider returned an invalid conversation identity after partial settings",
                                    operation_id,
                                    None,
                                )),
                                }
                            }
                            Err(ExternalProviderRuntimeError::InvalidSetting {
                                setting,
                                value,
                                advertised,
                                provider_session_id,
                                disposition,
                            }) => {
                                let target = SessionId::try_from(provider_session_id)
                                    .ok()
                                    .map(|session_id| SessionRef { endpoint, session_id });
                                let failure = invalid_setting_failure(
                                    operation_id,
                                    target.clone(),
                                    setting,
                                    value,
                                    advertised,
                                    disposition,
                                );
                                match (target, disposition) {
                                    (Some(target), acp_client_runtime::InvalidSettingSessionDisposition::RemainsCreated) => {
                                        ProviderOperationCompletion::FailureWithSession {
                                            failure,
                                            session_record: Box::new(provider_session_record(
                                                target,
                                                working_directory,
                                                requested_policy,
                                                created_by,
                                                approver,
                                                resume,
                                            )),
                                        }
                                    }
                                    _ => ProviderOperationCompletion::Failure(failure),
                                }
                            }
                            Err(error) => ProviderOperationCompletion::Failure(runtime_failure(
                                operation_id,
                                None,
                                error,
                            )),
                        }
                    });
                    Ok(admitted_submission(snapshot))
                }
            }
        })
    }

    fn load(
        &self,
        request: ConversationLoadRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        let operation_id = request.operation_id.clone();
        let failure_target = Some(request.target.clone());
        let backend = self.clone();
        retain_operation(operation_id, failure_target, async move {
            let target = request.target.clone();
            let (binding, runtime) = backend.runtime_binding(
                &target.endpoint,
                &request.operation_id,
                Some(target.clone()),
            )?;
            if !runtime.admission().supports_load {
                return Err(failure(
                    ConversationOperationFailureKind::UnsupportedCapability,
                    ConversationOperationFailureStage::Binding,
                    ProviderOperationEffect::None,
                    "provider runtime does not support conversation load",
                    request.operation_id,
                    Some(target),
                ));
            }
            let prepared = backend
                .prepare_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationLoad,
                    binding,
                    Some(&target),
                )
                .await?;
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { snapshot, live } => {
                    let operation_id = request.operation_id.clone();
                    let provider_session_id = String::from(target.session_id.clone());
                    let requested_policy = request.requested_policy;
                    let working_directory = request.working_directory;
                    let created_by = request.requested_by;
                    let approver = request.approver;
                    let completion_target = target.clone();
                    let completion_inner = Arc::clone(&backend.inner);
                    backend.spawn_operation(operation_id.clone(), live, async move {
                        let cwd = PathBuf::from(String::from(working_directory.clone()));
                        // The setup its create had, rebuilt from its record (D1 R18; Decision 3).
                        let setup = match crate::provider_session_setup::recorded_restore_setup(
                            &completion_inner.store,
                            runtime.session_setup(),
                            &completion_target,
                            &cwd,
                        )
                        .await
                        {
                            Ok(setup) => setup,
                            Err(_) => {
                                return ProviderOperationCompletion::Failure(failure(
                                    ConversationOperationFailureKind::Unavailable,
                                    ConversationOperationFailureStage::Dispatch,
                                    ProviderOperationEffect::None,
                                    "the session's record could not be read",
                                    operation_id,
                                    Some(completion_target),
                                ));
                            }
                        };
                        match runtime
                            .load_session_with_setup(provider_session_id.clone(), cwd, setup)
                            .await
                        {
                            Ok(()) => {
                                let mut observed = effective_settings(requested_policy.clone());
                                if let Some(catalog) =
                                    runtime.settings_catalog(&provider_session_id).await
                                {
                                    let effective = catalog.effective_settings();
                                    observed.mode = effective.mode;
                                    observed.model = effective.model;
                                    observed.effort = effective.effort;
                                    observed.fast = effective.fast.map(
                                        crate::provider_operation_settlement::provider_fast_state,
                                    );
                                    completion_inner
                                        .record_settings_catalog(completion_target.clone(), catalog)
                                        .await;
                                }
                                completion_inner
                                    .history_unavailable
                                    .lock()
                                    .await
                                    .remove(&completion_target);
                                ProviderOperationCompletion::Success {
                                    settlement: ConversationOperationSettlement::Loaded {
                                        target: completion_target.clone(),
                                        effective_settings: observed,
                                    },
                                    target: Some(completion_target.clone()),
                                    // A load writes none of the resume inputs, so the
                                    // ones its create recorded stay (Decision 3).
                                    session_record: Some(Box::new(provider_session_record(
                                        completion_target,
                                        working_directory,
                                        requested_policy,
                                        created_by,
                                        approver,
                                        collaboration_runtime::ProviderSessionResumeInputs::default(
                                        ),
                                    ))),
                                }
                            }
                            Err(error) => ProviderOperationCompletion::Failure(runtime_failure(
                                operation_id,
                                Some(completion_target),
                                error,
                            )),
                        }
                    });
                    Ok(admitted_submission(snapshot))
                }
            }
        })
    }

    fn prompt(
        &self,
        request: ConversationPromptRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        let operation_id = request.operation_id.clone();
        let target = Some(request.target.clone());
        match ProviderPromptContentsRequest::from_prompt_request(request) {
            Ok(contents_request) => self.prompt_contents(contents_request),
            Err(error) => retain_operation(operation_id, target, async move { Err(*error) }),
        }
    }

    fn cancel(
        &self,
        request: ConversationCancelRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSubmission> {
        let operation_id = request.operation_id.clone();
        let failure_target = Some(request.target.clone());
        let backend = self.clone();
        retain_operation(operation_id, failure_target, async move {
            let target = request.target.clone();
            let (binding, runtime) = backend.runtime_binding(
                &target.endpoint,
                &request.operation_id,
                Some(target.clone()),
            )?;
            if let Some(record) = backend
                .inner
                .store
                .lock()
                .await
                .inspect(&request.operation_id)
                .await
                .map_err(|_| {
                    unavailable_failure(request.operation_id.clone(), Some(target.clone()))
                })?
            {
                // A reused ID must name the same kind and session (D1 R15).
                if !super::provider_operation_admission::same_request(
                    &record,
                    ProviderOperationKind::ConversationCancel,
                    Some(&target),
                ) {
                    return Err(super::provider_operation_admission::reused_id_failure(
                        request.operation_id,
                        Some(target),
                    ));
                }
                return Ok(existing_submission(snapshot_from_record(record).map_err(
                    |message| snapshot_failure(request.operation_id, message),
                )?));
            }
            let target_operation = backend.inspect_record(&request.target_operation_id).await?;
            if target_operation.operation_kind != ProviderOperationKind::ConversationPrompt
                || target_operation.target.as_ref() != Some(&target)
                || !matches!(
                    &target_operation.binding,
                    ConversationBindingIdentity::ExternalProvider { binding }
                        if request.generation.as_ref() == Some(&binding.generation)
                )
                || target_operation.stage == ProviderOperationStage::Terminal
            {
                return Err(failure(
                    ConversationOperationFailureKind::NotFound,
                    ConversationOperationFailureStage::Validation,
                    ProviderOperationEffect::None,
                    "target provider prompt operation is not active on this binding",
                    request.operation_id,
                    Some(target),
                ));
            }
            let prepared = backend
                .prepare_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationCancel,
                    binding,
                    Some(&target),
                )
                .await?;
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { snapshot, live } => {
                    let operation_id = request.operation_id.clone();
                    let target_operation_id = request.target_operation_id;
                    let provider_session_id = String::from(target.session_id.clone());
                    let completion_target = target.clone();
                    // A prompt still waiting in its session's line is taken
                    // out of it and ends not sent; one already running or
                    // dispatched keeps the interrupt (D1 R12, R14).
                    let waiting_input = backend
                        .inner
                        .queued_operations
                        .snapshot(&target_operation_id)
                        .ok()
                        .flatten()
                        .filter(|queued| {
                            queued.queue_state
                                == Some(collaboration_protocol::ConversationOperationQueueState::RouterQueued)
                        })
                        .and_then(|queued| queued.input_id);
                    let dequeued = waiting_input.is_some_and(|input_id| {
                        backend
                            .message_fifo()
                            .is_some_and(|fifo| fifo.cancel_queued(&target, &input_id).is_ok())
                    });
                    backend.spawn_operation(operation_id.clone(), live, async move {
                        if dequeued {
                            return ProviderOperationCompletion::Success {
                                settlement: ConversationOperationSettlement::CancelRequested {
                                    target: completion_target.clone(),
                                    target_operation_id,
                                },
                                target: Some(completion_target),
                                session_record: None,
                            };
                        }
                        match runtime
                            .cancel_prompt_operation(
                                provider_session_id,
                                target_operation_id.clone(),
                            )
                            .await
                        {
                            Ok(()) => ProviderOperationCompletion::Success {
                                settlement: ConversationOperationSettlement::CancelRequested {
                                    target: completion_target.clone(),
                                    target_operation_id,
                                },
                                target: Some(completion_target),
                                session_record: None,
                            },
                            Err(error) => ProviderOperationCompletion::Failure(runtime_failure(
                                operation_id,
                                Some(completion_target),
                                error,
                            )),
                        }
                    });
                    Ok(admitted_submission(snapshot))
                }
            }
        })
    }

    fn show(
        &self,
        request: ConversationOperationShowRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSnapshot> {
        Box::pin(async move {
            let stored_record = self
                .inner
                .store
                .lock()
                .await
                .inspect(&request.operation_id)
                .await
                .map_err(|_| unavailable_failure(request.operation_id.clone(), None))?;
            if let Some(record) = stored_record {
                let mut snapshot = snapshot_from_record(record)
                    .map_err(|message| snapshot_failure(request.operation_id.clone(), message))?;
                // A waiting API prompt's row is Admitted; the session's line
                // knows it is queued, or why it wasn't sent (D1 R14).
                if snapshot.stage == collaboration_protocol::ProviderOperationStage::Admitted
                    && let Some(queued) = self
                        .inner
                        .queued_operations
                        .snapshot(&request.operation_id)
                        .map_err(|message| {
                            snapshot_failure(request.operation_id.clone(), message)
                        })?
                {
                    snapshot.queue_state = queued.queue_state;
                    snapshot.input_id = queued.input_id;
                }
                return Ok(snapshot);
            }
            if let Some(snapshot) = self
                .inner
                .queued_operations
                .snapshot(&request.operation_id)
                .map_err(|message| snapshot_failure(request.operation_id.clone(), message))?
            {
                return Ok(snapshot);
            }
            Err(failure(
                ConversationOperationFailureKind::NotFound,
                ConversationOperationFailureStage::Settlement,
                ProviderOperationEffect::None,
                "the Router has no record of this operation ID",
                request.operation_id,
                None,
            ))
        })
    }

    fn wait(
        &self,
        request: ConversationOperationWaitRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationWaitResult> {
        Box::pin(async move {
            let timeout_seconds = u32::from(request.timeout_seconds);
            let operation_id = request.operation_id;
            let live = self
                .inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .operations
                .get(&operation_id)
                .cloned();
            if let Some(live) = live {
                let deadline =
                    tokio::time::Instant::now() + Duration::from_secs(timeout_seconds.into());
                loop {
                    let changed = live.changed.notified();
                    if let Some(result) = live.result.lock().await.clone() {
                        let operation =
                            snapshot_from_record(self.inspect_record(&operation_id).await?)
                                .map_err(|message| {
                                    snapshot_failure(operation_id.clone(), message)
                                })?;
                        return result.map(|settlement| ConversationOperationWaitResult {
                            operation,
                            output: ConversationOperationWaitOutput::Available { settlement },
                        });
                    }
                    if tokio::time::timeout_at(deadline, changed).await.is_err() {
                        // Through show, so a queued prompt says so (D1 R12).
                        let operation = self
                            .show(ConversationOperationShowRequest {
                                operation_id: operation_id.clone(),
                            })
                            .await?;
                        return Ok(ConversationOperationWaitResult {
                            operation,
                            output: ConversationOperationWaitOutput::Pending,
                        });
                    }
                }
            }

            let record = self.inspect_record(&operation_id).await?;
            let output = if record.stage == ProviderOperationStage::Terminal {
                ConversationOperationWaitOutput::OutputUnavailable {
                    reason: if record.admitted_at_ms <= self.inner.started_at_ms {
                        ConversationOutputUnavailableReason::HostRestarted
                    } else {
                        ConversationOutputUnavailableReason::NotRetained
                    },
                }
            } else {
                ConversationOperationWaitOutput::Pending
            };
            let operation = snapshot_from_record(record)
                .map_err(|message| snapshot_failure(operation_id, message))?;
            Ok(ConversationOperationWaitResult { operation, output })
        })
    }

    fn reconcile(
        &self,
        request: ConversationOperationReconcileRequest,
    ) -> ProviderConversationFuture<'_, ConversationOperationSnapshot> {
        Box::pin(async move {
            // Admission reserves an operation in the live index before its
            // durable insert, so a row with no live owner here has none
            // (design note §15.3); no store lock is held across this read.
            let is_live = self
                .inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .operations
                .get(&request.operation_id)
                .map(|operation| match operation.result.try_lock() {
                    Ok(result) => result.is_none(),
                    Err(_) => true,
                })
                .unwrap_or(false);
            if is_live {
                // A reservation answers once its durable insert does.
                self.wait_for_reservation(&request.operation_id).await;
            }
            let mut store = self.inner.store.lock().await;
            if is_live {
                let record = store
                    .inspect(&request.operation_id)
                    .await
                    .map_err(|_| unavailable_failure(request.operation_id.clone(), None))?
                    .ok_or_else(|| {
                        failure(
                            ConversationOperationFailureKind::NotFound,
                            ConversationOperationFailureStage::Settlement,
                            ProviderOperationEffect::None,
                            "the Router has no record of this operation ID",
                            request.operation_id.clone(),
                            None,
                        )
                    })?;
                return snapshot_from_record(record)
                    .map_err(|message| snapshot_failure(request.operation_id, message));
            }
            let record = store
                .inspect(&request.operation_id)
                .await
                .map_err(|_| unavailable_failure(request.operation_id.clone(), None))?
                .ok_or_else(|| {
                    failure(
                        ConversationOperationFailureKind::NotFound,
                        ConversationOperationFailureStage::Settlement,
                        ProviderOperationEffect::None,
                        "the Router has no record of this operation ID",
                        request.operation_id.clone(),
                        None,
                    )
                })?;
            let record = if record.reconciliation_state == ProviderReconciliationState::Unresolved {
                store
                    .record_not_reconcilable(&request.operation_id, now_ms())
                    .await
                    .map_err(|_| unavailable_failure(request.operation_id.clone(), record.target))?
            } else {
                record
            };
            snapshot_from_record(record).map_err(|message| {
                failure(
                    ConversationOperationFailureKind::ProtocolViolation,
                    ConversationOperationFailureStage::Reconciliation,
                    ProviderOperationEffect::Unknown,
                    message,
                    request.operation_id,
                    None,
                )
            })
        })
    }
}

#[cfg(test)]
#[path = "provider_conversation_backend_test.rs"]
mod tests;
