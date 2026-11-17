//! Public provider Session operations after connection admission.
#[cfg(any(test, feature = "test-observation"))]
use super::{
    ExternalProviderApprovalRefusalWarning, ExternalProviderPermissionObservation,
    ExternalProviderPermissionOutcome, ExternalProviderToolCall,
};

use super::{
    AgentSessionClient, Arc, CancellationToken, ExternalProviderAdmission,
    ExternalProviderCreatedSession, ExternalProviderRuntimeError, InputId, InteractionPort,
    Ordering, PathBuf, ProviderCapabilityReport, ProviderCommand, ProviderPromptContent,
    ProviderSessionActivity, ProviderSteeringOutcome,
};
use crate::provider_prompt_content::acp_blocks_from_prompt_content;
use agent_client_protocol::schema::v1::ContentBlock;

/// A close the session's cell admitted. Until its Close command enters the
/// connection queue, dropping the caller settles it as failed, so the cell
/// returns to where it started; the connection settles it afterward.
struct PendingCloseAdmissionGuard {
    cells: Arc<crate::session_cell_table::SessionCellTable>,
    provider_session_id: String,
    attempt: provider_session_machines::session_lifecycle_machine::CloseAttempt,
    submitted: bool,
}

impl PendingCloseAdmissionGuard {
    /// Admits the close on the session's cell: Live or Loading to Closing.
    fn admit(
        cells: &Arc<crate::session_cell_table::SessionCellTable>,
        provider_session_id: &str,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let attempt = provider_session_machines::session_lifecycle_machine::CloseAttempt(
            uuid::Uuid::now_v7(),
        );
        crate::session_cell_bookkeeping::admit_close(cells, provider_session_id, attempt)?;
        Ok(Self {
            cells: Arc::clone(cells),
            provider_session_id: provider_session_id.to_owned(),
            attempt,
            submitted: false,
        })
    }

    fn transfer_to_connection(&mut self) {
        self.submitted = true;
    }
}

impl Drop for PendingCloseAdmissionGuard {
    fn drop(&mut self) {
        if !self.submitted {
            crate::session_cell_bookkeeping::settle_close(
                &self.cells,
                &self.provider_session_id,
                self.attempt,
                false,
            );
        }
    }
}

impl<P: InteractionPort> AgentSessionClient<P> {
    fn retired_operation_error(&self) -> ExternalProviderRuntimeError {
        if self.sink_closed.is_cancelled() {
            ExternalProviderRuntimeError::SinkClosed
        } else {
            ExternalProviderRuntimeError::TransportFailure
        }
    }

    #[must_use]
    pub fn admission(&self) -> &ExternalProviderAdmission {
        &self.admission
    }

    pub async fn capability_report(&self, provider_session_id: &str) -> ProviderCapabilityReport {
        let report = self
            .session_capabilities
            .read(provider_session_id)
            .unwrap_or_else(|| self.base_capabilities.clone());
        report.with_auth_status(self.auth_status.read().await.clone())
    }

    pub fn settings_catalog(
        &self,
        provider_session_id: &str,
    ) -> Option<crate::ProviderSettingsCatalog> {
        self.session_settings.read(provider_session_id)
    }

    /// A setting holds the session's prompts: refused where refusals hold, or
    /// its outcome unknown.
    pub fn settings_unresolved(&self, provider_session_id: &str) -> bool {
        super::settings_held(&self.session_cells, provider_session_id)
    }

    /// True while this setting's last change has an unknown outcome, so a
    /// reader never reports the catalog's old value as confirmed (D1 R2).
    pub fn setting_outcome_unknown(
        &self,
        provider_session_id: &str,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        crate::session_cell_bookkeeping::setting_outcome_unknown(
            &self.session_cells,
            provider_session_id,
            kind,
        )
    }

    pub fn accept_session_settings(
        &self,
        provider_session_id: String,
    ) -> Result<crate::EffectiveProviderSettings, ExternalProviderRuntimeError> {
        let catalog = self
            .session_settings
            .read(&provider_session_id)
            .ok_or(ExternalProviderRuntimeError::LocalNotFound)?;
        // One session-cell event across kinds: refused while any outcome is
        // unknown; an in-flight change is left alone, as today.
        let accepted = crate::session_cell_bookkeeping::accept_settings(
            &self.session_cells,
            &provider_session_id,
        )?;
        if accepted.contains(&provider_session_machines::ProviderSettingKind::Fast) {
            // The caller accepted a refused fast as it stands (D1 R8, R9).
            let run = crate::SessionRuns::current_generation(
                self.session_cells.as_ref(),
                &provider_session_id,
            );
            crate::session_cell_bookkeeping::set_keep_fast_off_in_run(
                &self.session_cells,
                &provider_session_id,
                run,
                false,
            );
        }
        Ok(catalog.effective_settings())
    }

    pub async fn set_setting(
        &self,
        provider_session_id: String,
        request: crate::ProviderSettingRequest,
    ) -> Result<crate::EffectiveProviderSettings, ExternalProviderRuntimeError> {
        let uncertain_session_id = provider_session_id.clone();
        let uncertain_value = request.value.clone();
        let kind = request.kind;
        // The run this change is sent to: a lost reply speaks only for it.
        let run = crate::SessionRuns::current_generation(
            self.session_cells.as_ref(),
            &provider_session_id,
        );
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::SetSetting {
                provider_session_id,
                request,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        match result.await {
            Ok(result) => result,
            Err(_) => {
                // The reply never came back: the change's outcome is unknown,
                // for the run it was sent to and no later one (Lead V4).
                crate::session_cell_bookkeeping::apply_setting_in_run(
                    &self.session_cells,
                    &uncertain_session_id,
                    run,
                    kind,
                    provider_session_machines::session_setting_machine::SessionSettingEvent::Lost,
                );
                Err(ExternalProviderRuntimeError::SettingOutcomeUnknown {
                    provider_session_id: uncertain_session_id,
                    setting: kind,
                    value: uncertain_value,
                })
            }
        }
    }

    #[must_use]
    #[cfg(any(test, feature = "test-observation"))]
    pub fn permission_observation(&self) -> ExternalProviderPermissionObservation {
        let request_count = self.permission_request_count.load(Ordering::Relaxed);
        ExternalProviderPermissionObservation {
            method: "session/request_permission",
            request_count,
            last_outcome: match self.permission_outcome.load(Ordering::Relaxed) {
                1 => Some(ExternalProviderPermissionOutcome::Cancelled),
                2 => Some(ExternalProviderPermissionOutcome::Selected),
                _ => None,
            },
            execute_tool_call_count: 0,
        }
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn take_test_tool_calls(&self) -> Vec<ExternalProviderToolCall> {
        std::mem::take(
            &mut *self
                .test_tool_calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    #[must_use]
    pub fn retirement(&self) -> CancellationToken {
        self.retirement.clone()
    }

    pub async fn set_endpoint_id(&self, endpoint_id: String) {
        *self.endpoint_id.write().await = Some(endpoint_id);
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn approval_refusal_warnings(&self) -> Vec<ExternalProviderApprovalRefusalWarning> {
        self.approval_refusal_warnings
            .lock()
            .map(|warnings| warnings.clone())
            .unwrap_or_default()
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn active_approval_operation(&self, provider_session_id: &str) -> Option<P::OperationId> {
        self.approval_contexts.lock().ok().and_then(|contexts| {
            contexts
                .get(provider_session_id)
                .map(|context| P::operation_id(&context.approval))
        })
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn active_approval_count(&self) -> usize {
        self.approval_contexts
            .lock()
            .map_or(0, |contexts| contexts.len())
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub async fn abort_owner_for_test(&self) {
        if let Some(task) = self.task.lock().await.as_ref() {
            task.abort();
        }
    }

    pub async fn create_session(
        &self,
        cwd: PathBuf,
    ) -> Result<String, ExternalProviderRuntimeError> {
        self.create_session_with_observation(cwd)
            .await
            .map(|created| created.provider_session_id)
    }

    pub async fn create_session_with_observation(
        &self,
        cwd: PathBuf,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.create_session_with_settings(cwd, None, crate::RequestedProviderSettings::default())
            .await
    }

    pub async fn create_session_with_settings(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        settings: crate::RequestedProviderSettings,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.create_session_with_work_roots(
            cwd,
            scratch_folder,
            settings,
            crate::SessionWorkRoots::default(),
        )
        .await
    }

    /// Creates a session that also works in the Router's work roots (D1 R18).
    pub async fn create_session_with_work_roots(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        settings: crate::RequestedProviderSettings,
        work_roots: crate::SessionWorkRoots,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.create_session_admitted(cwd, scratch_folder, settings, work_roots)
            .await
            .0
    }

    /// Creates a session; answers its outcome and, when the connection
    /// admitted it (even without its settings), the generation it was
    /// admitted under, which the caller's own publications for it carry.
    pub async fn create_session_admitted(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        settings: crate::RequestedProviderSettings,
        work_roots: crate::SessionWorkRoots,
    ) -> (
        Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError>,
        Option<provider_session_machines::SessionGeneration>,
    ) {
        let (reply, result) = tokio::sync::oneshot::channel();
        if self
            .commands
            .send(ProviderCommand::Create {
                cwd,
                scratch_folder,
                work_roots,
                settings,
                reply,
            })
            .await
            .is_err()
        {
            return (Err(self.retired_operation_error()), None);
        }
        match result.await {
            Ok(super::CreateAdmissionReply { result, generation }) => (result, generation),
            Err(_) => (Err(self.retired_operation_error()), None),
        }
    }

    pub async fn load_session(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
    ) -> Result<(), ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_load {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/load",
            });
        }
        self.load_session_with_setup(
            provider_session_id,
            cwd,
            crate::SessionRestoreSetup::default(),
        )
        .await
        .map(|_| ())
    }

    /// Loads the session, rebuilding the setup its create had (D1 R18);
    /// answers the generation the connection admitted it under, which the
    /// caller's own publications for this run carry, and its mode report.
    pub async fn load_session_with_setup(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
        setup: crate::SessionRestoreSetup,
    ) -> Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_load {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/load",
            });
        }
        self.restore_session(
            provider_session_id,
            cwd,
            super::provider_session_restore::RestoreHistoryMode::Replay,
            setup,
        )
        .await
    }

    pub async fn resume_session(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
    ) -> Result<(), ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_resume {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/resume",
            });
        }
        self.resume_session_with_setup(
            provider_session_id,
            cwd,
            crate::SessionRestoreSetup::default(),
        )
        .await
        .map(|_| ())
    }

    /// Resumes the session, rebuilding the setup its create had (D1 R18);
    /// answers the generation the connection admitted it under and its mode
    /// report.
    pub async fn resume_session_with_setup(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
        setup: crate::SessionRestoreSetup,
    ) -> Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_resume {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/resume",
            });
        }
        self.restore_session(
            provider_session_id,
            cwd,
            super::provider_session_restore::RestoreHistoryMode::WithoutReplay,
            setup,
        )
        .await
    }

    async fn restore_session(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
        mode: super::provider_session_restore::RestoreHistoryMode,
        setup: crate::SessionRestoreSetup,
    ) -> Result<super::RestoredSessionAdmission, ExternalProviderRuntimeError> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::Restore {
                provider_session_id,
                cwd,
                mode,
                setup,
                admitted: None,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())?
    }

    /// Test support: ends the session's actor the way an actor ends on its
    /// own (its command channel closes), without closing the session.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn end_session_actor(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.commands
            .send(ProviderCommand::EndActor {
                provider_session_id,
            })
            .await
            .map_err(|_| self.retired_operation_error())
    }

    pub async fn list_sessions(
        &self,
        cwd: Option<PathBuf>,
    ) -> Result<Vec<super::ProviderSessionSummary>, ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_list {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/list",
            });
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::List { cwd, reply })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())?
    }

    pub async fn close_session(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_close {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/close",
            });
        }
        let mut admission_guard =
            PendingCloseAdmissionGuard::admit(&self.session_cells, &provider_session_id)?;
        self.close_session_after_mark(provider_session_id, false, &mut admission_guard)
            .await
    }

    async fn close_session_after_mark(
        &self,
        provider_session_id: String,
        release_only: bool,
        admission_guard: &mut PendingCloseAdmissionGuard,
    ) -> Result<(), ExternalProviderRuntimeError> {
        if self.session_activity(provider_session_id.clone()).await?
            == ProviderSessionActivity::Running
        {
            match self.cancel_active_prompt(provider_session_id.clone()).await {
                Ok(()) | Err(ExternalProviderRuntimeError::LocalNotFound) => {}
                Err(error) => return Err(error),
            }
            self.wait_session_idle(provider_session_id.clone()).await?;
        }
        // A close that found a turn running frees the session only once that
        // turn's end is recorded on the cell (design note §12.1, EndingTurn).
        tokio::select! {
            () = self.shutdown.cancelled() => return Err(self.retired_operation_error()),
            () = crate::session_cell_bookkeeping::wait_for_close_turn(&self.session_cells, &provider_session_id) => {}
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::Close {
                provider_session_id,
                attempt: admission_guard.attempt,
                release_only,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        admission_guard.transfer_to_connection();
        result.await.map_err(|_| self.retired_operation_error())?
    }

    /// Closes a session on an agent that offers no session/close (D1 R14):
    /// interrupts a running turn, waits for idle, then releases the session
    /// here. The agent keeps no close of its own.
    pub async fn release_session(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let mut admission_guard =
            PendingCloseAdmissionGuard::admit(&self.session_cells, &provider_session_id)?;
        self.close_session_after_mark(provider_session_id, true, &mut admission_guard)
            .await
    }

    /// Steer with the same validated Session vocabulary used for prompts.
    /// Optional ACP content types are checked before any command is queued.
    pub async fn steer_contents_with_input(
        &self,
        provider_session_id: String,
        input_id: InputId,
        submission: session_event_model::ProviderSubmission,
    ) -> Result<ProviderSteeringOutcome<P::OperationId, P::Context>, ExternalProviderRuntimeError>
    {
        if !submission.selected.is_empty() {
            return Err(ExternalProviderRuntimeError::Behavior(
                crate::ProviderError::Unsupported,
            ));
        }
        self.steer_acp_blocks_with_input(
            provider_session_id,
            input_id,
            acp_blocks_from_prompt_content(submission.content),
        )
        .await
    }

    async fn steer_acp_blocks_with_input(
        &self,
        provider_session_id: String,
        input_id: InputId,
        blocks: Vec<ContentBlock>,
    ) -> Result<ProviderSteeringOutcome<P::OperationId, P::Context>, ExternalProviderRuntimeError>
    {
        if !self.admission.supports_steering {
            return Err(ExternalProviderRuntimeError::UnsupportedSteering);
        }
        let capabilities = self.capability_report(&provider_session_id).await;
        let prompt = ProviderPromptContent::new(blocks, &capabilities)?;
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::Steer {
                provider_session_id,
                input_id,
                prompt,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())?
    }

    pub async fn session_activity(
        &self,
        provider_session_id: String,
    ) -> Result<ProviderSessionActivity, ExternalProviderRuntimeError> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::InspectSession {
                provider_session_id,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())
    }

    /// Resolve the operation owned by the currently running Session turn.
    pub async fn active_prompt_operation(
        &self,
        provider_session_id: String,
    ) -> Result<Option<P::OperationId>, ExternalProviderRuntimeError> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::InspectActiveOperation {
                provider_session_id,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())
    }

    pub async fn wait_session_idle(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand::WaitSessionIdle {
                provider_session_id,
                reply,
            })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())?
    }

    pub async fn shutdown(&self) {
        self.retirement.cancel();
        self.shutdown.cancel();
        if let Some(task) = self.task.lock().await.take()
            && task.await.is_err()
        {
            self.shutdown_failed.store(true, Ordering::Relaxed);
        }
    }

    #[must_use]
    pub fn shutdown_failed(&self) -> bool {
        self.shutdown_failed.load(Ordering::Relaxed)
    }
}
