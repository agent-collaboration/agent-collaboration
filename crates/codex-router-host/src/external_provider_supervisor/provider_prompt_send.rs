//! The API prompt's one send behaviour (D1 R12, Changes 4 and 8): under the
//! caller's one operation ID and delivery choice, an idle session is
//! prompted. A busy one is steered when the agent can steer. When it can't,
//! auto refuses with CannotSteer (nothing sent; the caller chooses queue or an
//! interrupt), queue waits in the session's line, and steer is refused. Steer
//! and queue mirror message send's provider rules. A send never interrupts a
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
use collaboration_protocol::MessageDelivery;

/// Where one look at the session sends the prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendRoute {
    Prompt,
    Steer,
    Queue,
    /// Nothing is sent; the operation settles with this refusal.
    Refuse(SendRefusal),
}

/// Why a prompt was not sent, decided by the session and the caller's choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendRefusal {
    /// Auto, busy, and the agent can't take input mid-turn.
    CannotSteer,
    /// Steer, and the agent can't steer at all.
    SteerUnsupported,
    /// Steer, and no turn is running.
    NoRunningTurn,
}

impl SendRefusal {
    fn failure(
        self,
        operation_id: OperationId,
        target: super::SessionRef,
    ) -> ConversationOperationFailure {
        let (kind, message) = match self {
            Self::CannotSteer => (
                ConversationOperationFailureKind::CannotSteer,
                CANNOT_STEER_MESSAGE,
            ),
            Self::SteerUnsupported => (
                ConversationOperationFailureKind::UnsupportedCapability,
                "steer unsupported by provider",
            ),
            Self::NoRunningTurn => (
                ConversationOperationFailureKind::InvalidRequest,
                "no running turn to steer",
            ),
        };
        failure(
            kind,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::None,
            message,
            operation_id,
            Some(target),
        )
    }
}

/// What a caller with no choice is told when the agent can't steer (D1 R12).
const CANNOT_STEER_MESSAGE: &str = "cannot steer: the agent can't take input while its turn runs, so nothing was sent. Send again with delivery queue to wait for the turn to end, or interrupt the turn (conversation cancel) and then send";

impl ExternalProviderSupervisor {
    /// Records the approver of a prompt this Host dispatched, kept while its
    /// settlement is retained.
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

    /// The approver recorded for a turn this Host dispatched.
    fn turn_approver_of(&self, operation_id: &OperationId) -> Option<ProviderIdentity> {
        self.inner
            .turn_approvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(operation_id)
            .cloned()
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

    /// The caller's choice and the session's state decide the route. Queue
    /// always waits in the line; steer needs an agent that steers and a
    /// running turn; auto prompts an idle session and steers a busy one, or
    /// says cannot steer.
    async fn send_route(
        runtime: &Arc<crate::ExternalProviderRuntime>,
        provider_session_id: &str,
        delivery: MessageDelivery,
    ) -> SendRoute {
        if delivery == MessageDelivery::Queue {
            return SendRoute::Queue;
        }
        if delivery == MessageDelivery::Steer && !Self::steers(runtime, provider_session_id).await {
            return SendRoute::Refuse(SendRefusal::SteerUnsupported);
        }
        match runtime
            .session_activity(provider_session_id.to_owned())
            .await
        {
            Ok(acp_client_runtime::ProviderSessionActivity::Running) => {
                Self::busy_route(runtime, provider_session_id, delivery).await
            }
            // Idle, not loaded, or unreadable: the dispatch itself decides.
            _ if delivery == MessageDelivery::Steer => {
                SendRoute::Refuse(SendRefusal::NoRunningTurn)
            }
            _ => SendRoute::Prompt,
        }
    }

    async fn busy_route(
        runtime: &Arc<crate::ExternalProviderRuntime>,
        provider_session_id: &str,
        delivery: MessageDelivery,
    ) -> SendRoute {
        match (delivery, Self::steers(runtime, provider_session_id).await) {
            (MessageDelivery::Queue, _) => SendRoute::Queue,
            (_, true) => SendRoute::Steer,
            (MessageDelivery::Auto, false) => SendRoute::Refuse(SendRefusal::CannotSteer),
            (MessageDelivery::Steer, false) => SendRoute::Refuse(SendRefusal::SteerUnsupported),
        }
    }

    /// Where a send goes once the session changed twice between a look and
    /// a send: auto reached here only through steering, so it queues (C4,
    /// step 6); an explicit steer is refused, as there is no turn to join.
    const fn settled_route(delivery: MessageDelivery) -> SendRoute {
        match delivery {
            MessageDelivery::Auto | MessageDelivery::Queue => SendRoute::Queue,
            MessageDelivery::Steer => SendRoute::Refuse(SendRefusal::NoRunningTurn),
        }
    }

    async fn steers(
        runtime: &Arc<crate::ExternalProviderRuntime>,
        provider_session_id: &str,
    ) -> bool {
        runtime
            .capability_report(provider_session_id)
            .await
            .supports_steering
    }

    pub(super) fn send_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        delivery: MessageDelivery,
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
            let mut route = Self::send_route(&runtime, &provider_session_id, delivery).await;
            // One re-decision when the session changed between the look and
            // the send; a second change takes the settled route (C4, step 6).
            for attempt in 0..2 {
                match route {
                    SendRoute::Refuse(refusal) => {
                        let refused = refusal.failure(request.operation_id.clone(), target.clone());
                        let settled = refused.clone();
                        backend.spawn_operation(request.operation_id.clone(), live, async move {
                            ProviderOperationCompletion::Failure(settled)
                        });
                        return Err(refused);
                    }
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
                                    Self::busy_route(&runtime, &provider_session_id, delivery).await
                                } else {
                                    Self::settled_route(delivery)
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
                            route = if attempt == 0 && delivery == MessageDelivery::Auto {
                                SendRoute::Prompt
                            } else {
                                Self::settled_route(delivery)
                            };
                        }
                    },
                    SendRoute::Queue => {
                        #[cfg(test)]
                        {
                            let pause = backend
                                .inner
                                .queue_enqueue_test_pause
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .take();
                            if let Some(pause) = pause {
                                let _entered = pause.entered.send(());
                                let _released = pause.release.await;
                            }
                        }
                        return backend
                            .queue_admitted_api_prompt(request, &binding, snapshot, live)
                            .await;
                    }
                }
            }
            match Self::settled_route(delivery) {
                SendRoute::Refuse(refusal) => {
                    let refused = refusal.failure(request.operation_id.clone(), target);
                    let settled = refused.clone();
                    backend.spawn_operation(request.operation_id, live, async move {
                        ProviderOperationCompletion::Failure(settled)
                    });
                    Err(refused)
                }
                _ => {
                    backend
                        .queue_admitted_api_prompt(request, &binding, snapshot, live)
                        .await
                }
            }
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
        self.begin_dispatch(&request.operation_id, &target).await?;
        // Every turn's approver record stays from before the injection until
        // this steer reads the one it joined, whichever turn that turns out
        // to be and however many settlements are evicted meanwhile.
        let _approvers_pinned = self.pin_turn_approvers();
        #[cfg(test)]
        {
            let pause = self
                .inner
                .steer_injection_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                let _entered = pause.entered.send(());
                let _released = pause.release.await;
            }
        }
        let steered = runtime
            .steer_session_contents_with_input(
                provider_session_id.clone(),
                request.input_id.clone(),
                submission,
            )
            .await;
        #[cfg(test)]
        {
            let pause = self
                .inner
                .steer_reply_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                let _entered = pause.entered.send(());
                let _released = pause.release.await;
            }
        }
        let operation_id = request.operation_id.clone();
        let completion = match steered {
            Ok(crate::ProviderSteeringOutcome::Injected {
                running_operation_id,
            }) => {
                // The approver of the turn the input actually joined, as the
                // injection reports it, when it differs from this request's;
                // its record is pinned until this read.
                let turn_approver = running_operation_id
                    .and_then(|running| self.turn_approver_of(&running))
                    .filter(|approver| approver != &request.approver);
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
    async fn queue_admitted_api_prompt(
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
        match fifo
            .enqueue_admitted_api_prompt(request, binding.clone(), admitted.clone())
            .await
        {
            Ok(_queued) => Ok(admitted_submission(snapshot)),
            // The session's state refused it at the line's door (M8).
            Err(
                crate::provider_acp_message_fifo::QueueDoorRefusal::NotLoaded(answer)
                | crate::provider_acp_message_fifo::QueueDoorRefusal::Refused(answer),
            ) => {
                self.complete_unsent_api_entry(&admitted, &operation_id, &target, &answer);
                Err(refused(
                    ConversationOperationFailureKind::InvalidRequest,
                    &format!("not sent: {answer}"),
                ))
            }
            Err(crate::provider_acp_message_fifo::QueueDoorRefusal::Busy(_full)) => {
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
