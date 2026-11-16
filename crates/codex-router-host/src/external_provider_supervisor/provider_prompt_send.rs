//! The API prompt's one send behaviour (D1 R12, Change 4): under the caller's
//! one operation ID, an idle session is prompted, a busy one that can steer
//! is steered, and a busy one that can't is queued. A send never interrupts a
//! turn and never answers "busy" for a running turn.

use super::provider_operation_admission::DispatchMark;
use super::provider_prompt_contents::{AdmittedPromptDispatch, ProviderPromptContentsRequest};
use super::{
    Arc, ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement,
    ConversationOperationSnapshot, ConversationOperationSubmission, ExternalProviderSupervisor,
    LiveOperation, OperationId, PreparedOperation, ProviderConversationFuture, ProviderIdentity,
    ProviderOperationCompletion, ProviderOperationEffect, ProviderOperationKind,
    admitted_submission, existing_submission, failure, retain_operation,
};

/// Where one look at the session sends the prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendRoute {
    Prompt,
    Steer,
    Queue,
}

impl ExternalProviderSupervisor {
    /// Records the approver of a prompt this Host dispatched, until it settles.
    pub(super) fn remember_turn_approver(
        &self,
        operation_id: &OperationId,
        approver: &ProviderIdentity,
    ) {
        self.inner
            .turn_approvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(operation_id.clone(), approver.clone());
    }

    pub(super) fn forget_turn_approver(&self, operation_id: &OperationId) {
        self.inner
            .turn_approvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(operation_id);
    }

    /// Gives the supervisor the session line API prompts wait in. Called once,
    /// when the delivery route is composed.
    pub(crate) fn attach_message_fifo(
        &self,
        fifo: &Arc<crate::provider_acp_message_fifo::ProviderAcpMessageFifo>,
    ) {
        let _already = self.inner.message_fifo.set(Arc::downgrade(fifo));
    }

    pub(super) fn message_fifo(
        &self,
    ) -> Option<Arc<crate::provider_acp_message_fifo::ProviderAcpMessageFifo>> {
        self.inner
            .message_fifo
            .get()
            .and_then(std::sync::Weak::upgrade)
    }

    /// The session's state decides the route: idle prompts, a running turn
    /// steers when the agent advertises steering and queues when it doesn't.
    async fn send_route(
        runtime: &Arc<crate::ExternalProviderRuntime>,
        provider_session_id: &str,
    ) -> SendRoute {
        match runtime
            .session_activity(provider_session_id.to_owned())
            .await
        {
            Ok(acp_client_runtime::ProviderSessionActivity::Running) => {
                Self::busy_route(runtime, provider_session_id).await
            }
            // Idle, not loaded, or unreadable: the dispatch itself decides.
            _ => SendRoute::Prompt,
        }
    }

    async fn busy_route(
        runtime: &Arc<crate::ExternalProviderRuntime>,
        provider_session_id: &str,
    ) -> SendRoute {
        if runtime
            .capability_report(provider_session_id)
            .await
            .supports_steering
        {
            SendRoute::Steer
        } else {
            SendRoute::Queue
        }
    }

    pub(super) fn send_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, ConversationOperationSubmission> {
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
            let prepared = backend
                .admit_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationPrompt,
                    binding.clone(),
                    Some(&target),
                    DispatchMark::Deferred,
                )
                .await?;
            let (snapshot, live) = match prepared {
                PreparedOperation::Existing(operation) => {
                    return Ok(existing_submission(operation));
                }
                PreparedOperation::Admitted { snapshot, live } => (snapshot, live),
            };
            let provider_session_id = String::from(target.session_id.clone());
            let mut route = Self::send_route(&runtime, &provider_session_id).await;
            // One re-decision when the session changed between the look and
            // the send; a second change queues (C4, step 6).
            for attempt in 0..2 {
                match route {
                    SendRoute::Prompt => {
                        let dispatching = backend
                            .begin_dispatch(&request.operation_id, &target)
                            .await?;
                        match backend
                            .dispatch_admitted_prompt(
                                request.clone(),
                                &binding,
                                runtime.clone(),
                                dispatching,
                                Arc::clone(&live),
                            )
                            .await
                        {
                            AdmittedPromptDispatch::Done(result) => return *result,
                            AdmittedPromptDispatch::TurnRunning { .. } => {
                                route = if attempt == 0 {
                                    Self::busy_route(&runtime, &provider_session_id).await
                                } else {
                                    SendRoute::Queue
                                };
                            }
                        }
                    }
                    SendRoute::Steer => match backend
                        .steer_admitted(&request, &runtime, &snapshot, Arc::clone(&live))
                        .await?
                    {
                        SteerResult::Settled(submission) => return Ok(*submission),
                        SteerResult::NoRunningTurn => {
                            route = if attempt == 0 {
                                SendRoute::Prompt
                            } else {
                                SendRoute::Queue
                            };
                        }
                    },
                    SendRoute::Queue => {
                        return backend
                            .queue_admitted_api_prompt(request, &binding, snapshot, live);
                    }
                }
            }
            backend.queue_admitted_api_prompt(request, &binding, snapshot, live)
        })
    }

    /// Steers the prompt into the running turn with the caller's input ID.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    async fn steer_admitted(
        &self,
        request: &ProviderPromptContentsRequest,
        runtime: &Arc<crate::ExternalProviderRuntime>,
        snapshot: &ConversationOperationSnapshot,
        live: Arc<LiveOperation>,
    ) -> Result<SteerResult, ConversationOperationFailure> {
        let acp_client_runtime::ProviderTurnInput::Prompt(submission) = request.input.clone()
        else {
            return Ok(SteerResult::NoRunningTurn);
        };
        let target = request.target.clone();
        let provider_session_id = String::from(target.session_id.clone());
        let turn_approver = match runtime
            .active_prompt_operation(provider_session_id.clone())
            .await
        {
            Ok(Some(running)) => self
                .inner
                .turn_approvers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&running)
                .cloned()
                .filter(|approver| approver != &request.approver),
            _ => None,
        };
        self.begin_dispatch(&request.operation_id, &target).await?;
        let steered = runtime
            .steer_session_contents_with_input(
                provider_session_id.clone(),
                request.input_id.clone(),
                submission,
            )
            .await;
        let operation_id = request.operation_id.clone();
        let completion = match steered {
            Ok(crate::ProviderSteeringOutcome::Injected { .. }) => {
                ProviderOperationCompletion::Success {
                    settlement: ConversationOperationSettlement::Steered {
                        target: target.clone(),
                        turn_approver,
                    },
                    target: Some(target.clone()),
                    session_record: None,
                }
            }
            Ok(crate::ProviderSteeringOutcome::PromptRequired) => {
                // Nothing was injected: the row waits again rather than stay Unknown.
                self.undo_dispatch_mark(&operation_id, &target).await?;
                return Ok(SteerResult::NoRunningTurn);
            }
            Ok(crate::ProviderSteeringOutcome::StartedNewTurn) => {
                // Only an agent ignoring idleBehavior promptRequired starts a
                // turn: it ran, but its text isn't observed on this path.
                let runtime = Arc::clone(runtime);
                let completion_target = target.clone();
                self.spawn_operation(operation_id, live, async move {
                    let _idle = runtime.wait_session_idle(provider_session_id).await;
                    ProviderOperationCompletion::Success {
                        settlement: ConversationOperationSettlement::PromptCompleted {
                            target: completion_target.clone(),
                            stop_reason: collaboration_protocol::ProviderPromptStopReason::Unknown(
                                "startedFromSteer".to_owned(),
                            ),
                            response: None,
                        },
                        target: Some(completion_target),
                        session_record: None,
                    }
                });
                return Ok(SteerResult::Settled(Box::new(admitted_submission(
                    snapshot.clone(),
                ))));
            }
            Ok(crate::ProviderSteeringOutcome::Failed) => {
                ProviderOperationCompletion::Failure(failure(
                    ConversationOperationFailureKind::Unavailable,
                    ConversationOperationFailureStage::Dispatch,
                    ProviderOperationEffect::None,
                    "provider refused the steered input",
                    operation_id.clone(),
                    Some(target.clone()),
                ))
            }
            // An unknown steer outcome stays unknown and is never re-sent.
            Err(_) => ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::OutcomeUnknown,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::Unknown,
                "provider steer outcome is unknown",
                operation_id.clone(),
                Some(target.clone()),
            )),
        };
        self.spawn_operation(operation_id, live, async move { completion });
        Ok(SteerResult::Settled(Box::new(admitted_submission(
            snapshot.clone(),
        ))))
    }

    /// Hands the admitted prompt to the session's line, where it waits for the
    /// running turn to end. A full line refuses it, and its operation is
    /// settled unsent; without a line (a bare supervisor) the running turn
    /// refuses it as before.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    fn queue_admitted_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: &super::ProviderBindingIdentity,
        snapshot: ConversationOperationSnapshot,
        live: Arc<LiveOperation>,
    ) -> Result<ConversationOperationSubmission, ConversationOperationFailure> {
        let operation_id = request.operation_id.clone();
        let target = request.target.clone();
        let admitted = super::AdmittedApiPrompt::new(live);
        let refused = |kind, message: &str| {
            failure(
                kind,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::None,
                message,
                operation_id.clone(),
                Some(target.clone()),
            )
        };
        let Some(fifo) = self.message_fifo() else {
            self.complete_unsent_api_entry(
                &admitted,
                &operation_id,
                &target,
                "provider session is busy",
            );
            return Err(refused(
                ConversationOperationFailureKind::Busy,
                "provider session is busy",
            ));
        };
        match fifo.enqueue_admitted_api_prompt(request, binding.clone(), admitted.clone()) {
            Ok(_queued) => Ok(admitted_submission(snapshot)),
            Err(_full) => {
                self.complete_unsent_api_entry(&admitted, &operation_id, &target, "queue full");
                Err(refused(
                    ConversationOperationFailureKind::Overloaded,
                    "queue full",
                ))
            }
        }
    }
}

/// What a steer came to.
enum SteerResult {
    /// The operation is settled or settling; this is its submission.
    Settled(Box<ConversationOperationSubmission>),
    /// The turn ended first; nothing was sent.
    NoRunningTurn,
}
