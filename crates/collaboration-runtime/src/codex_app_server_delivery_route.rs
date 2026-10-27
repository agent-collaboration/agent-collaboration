//! Codex app-server delivery owns native admission and its recorded effects.
use crate::EndpointDirectory;
use crate::NativeControlBackend;
use crate::native_message_dispatch::{
    NativeMessageBody, NativeMessageParams, NativeThreadStatus, NativeThreadStatusReadError,
    read_native_thread_status,
};
use agent_automation::{
    CessationEvidence, NativeEffectEvidence, PreparationEffect, RouteEffectEvidence,
    SubmissionEffect,
};
use codex_acp_adapter::{HeldBindingCheckout, UnmaterializedBindingStore};
use codex_native_integration::NativeProtocolConnection;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::RouteUnavailableReason;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_protocol::{
    ChannelDescription, DeliveryCorrelationId, DeliveryRejectionReason, MachineId, MessageDelivery,
    NativeSendAcceptance, RouterLink, SessionReachability, SessionRef, UuidIdentity,
};
use serde_json::{Value, json};
use std::time::Duration;

pub struct CodexAppServerDeliveryRoute {
    service_id: UuidIdentity,
    endpoints: EndpointDirectory,
    backend: NativeControlBackend,
    holder: std::sync::Arc<crate::UnmaterializedThreadHolder>,
    approval_broker: Option<std::sync::Weak<dyn codex_acp_adapter::ApprovalBroker>>,
}

struct NativeRouteDeliveryRequest {
    target: SessionRef,
    body: NativeMessageBody,
    mode: MessageDelivery,
    load_policy: LoadPolicy,
    precondition: DeliveryPrecondition,
    correlation: DeliveryCorrelationId,
    attempt: agent_automation::AttemptId,
}

impl CodexAppServerDeliveryRoute {
    #[must_use]
    pub fn new(
        service_id: UuidIdentity,
        endpoints: EndpointDirectory,
        backend: NativeControlBackend,
        holder: std::sync::Arc<crate::UnmaterializedThreadHolder>,
    ) -> Self {
        Self {
            service_id,
            endpoints,
            backend,
            holder,
            approval_broker: None,
        }
    }

    /// Borrows the existing route owner without retaining the broker's delivery cycle.
    #[must_use]
    pub fn with_approval_broker(
        mut self,
        broker: &std::sync::Arc<crate::ServiceInteractionBroker>,
    ) -> Self {
        let broker_port: std::sync::Arc<dyn codex_acp_adapter::ApprovalBroker> = broker.clone();
        self.approval_broker = Some(std::sync::Arc::downgrade(&broker_port));
        self
    }

    async fn deliver_native(
        &self,
        request: NativeRouteDeliveryRequest,
        sink: &dyn AttemptEvidenceSink,
    ) -> Result<collaboration_contracts::NativeDeliveryResult, DeliveryContractError> {
        let Ok(admission) = self.backend.gate.acquire() else {
            return Ok(not_submitted(
                "Native backend unavailable",
                true,
                collaboration_contracts::UnsentCause::Holdable {
                    otherwise: collaboration_contracts::NativeRefusalCode::NativeUnavailable,
                },
            ));
        };
        if let DeliveryPrecondition::EndpointGeneration { expected } = &request.precondition
            && expected != admission.generation()
        {
            return Ok(not_submitted(
                "staleGeneration",
                false,
                collaboration_contracts::UnsentCause::Refused(
                    collaboration_contracts::NativeRefusalCode::Rejection(
                        DeliveryRejectionReason::StaleGeneration,
                    ),
                ),
            ));
        }
        let generation = admission.generation().clone();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let retirement = admission.retirement();
        let recorded_route = tokio::select! {
            biased;
            () = retirement.cancelled() => crate::native_recorded_policy::RecordedNativeRouteClassification::UnresolvedAuthority,
            result = tokio::time::timeout_at(deadline, crate::native_recorded_policy::classify_recorded_native_route(self.approval_broker.as_ref(), &request.target)) => {
                result.unwrap_or(crate::native_recorded_policy::RecordedNativeRouteClassification::UnresolvedAuthority)
            }
        };
        let mut effects = NativeEffectEvidence {
            permission_differences: Vec::new(),
            target: Some(request.target.clone()),
            generation: Some(generation.clone()),
            client_user_message_id: Some(request.correlation.as_str().to_owned()),
            native_turn_id: None,
            native_submission_id: None,
            allocation: PreparationEffect::NotRequested,
            // This same classification pins dispatch; a generic queue cannot attach later.
            resume: if request.mode == MessageDelivery::Auto || recorded_route.may_resume() {
                PreparationEffect::Unknown
            } else {
                PreparationEffect::NotRequested
            },
            submission: SubmissionEffect::Dispatching,
            cessation: CessationEvidence::NotApplicable,
        };
        sink.record(RouteEffectEvidence::CodexAppServer(effects.clone()))
            .await
            .map_err(|_| DeliveryContractError::EvidencePersistence)?;
        let endpoints = self
            .endpoints
            .subscribe()
            .and_then(|subscription| subscription.snapshot())
            .map_err(|_| DeliveryContractError::ClientOperation)?
            .endpoints;
        let load_policy = request.load_policy;
        let params = NativeMessageParams {
            target: request.target.clone(),
            generation: generation.clone(),
            body: request.body,
            delivery: request.mode,
            client_user_message_id: Some(
                request
                    .correlation
                    .as_str()
                    .to_owned()
                    .try_into()
                    .map_err(|_| DeliveryContractError::InvalidEvidence)?,
            ),
        };
        let checked_out = self
            .holder
            .checkout(&String::from(request.target.session_id.clone()));
        let (response, held_idle_submission) = match checked_out {
            HeldBindingCheckout::Ready(mut binding) => {
                let mut checkout = crate::unmaterialized_thread_holder::HeldBindingCleanup::new(
                    &self.holder,
                    binding.session_id(),
                );
                if binding.generation() != &generation {
                    checkout.finish();
                    effects.submission = SubmissionEffect::NotDispatched;
                    effects.resume = PreparationEffect::NotRequested;
                    sink.record(RouteEffectEvidence::CodexAppServer(effects))
                        .await?;
                    return Ok(not_submitted(
                        "staleGeneration",
                        false,
                        collaboration_contracts::UnsentCause::Refused(
                            collaboration_contracts::NativeRefusalCode::Rejection(
                                DeliveryRejectionReason::StaleGeneration,
                            ),
                        ),
                    ));
                }
                effects.permission_differences = binding.permission_differences().to_vec();
                sink.record(RouteEffectEvidence::CodexAppServer(effects.clone()))
                    .await?;
                let mut response = crate::native_message_dispatch::dispatch_message(
                    crate::native_message_dispatch::NativeMessageRequest {
                        attempt_evidence: Some(
                            crate::message_effect_state::NativeMessageAttemptEvidence {
                                effects: &mut effects,
                                sink,
                            },
                        ),
                        params,
                        id: json!(request.attempt.as_str()),
                        service_id: &self.service_id,
                        backend: &self.backend,
                        endpoints: &endpoints,
                        approval_broker: self.approval_broker.as_ref(),
                        recorded_route: Some(&recorded_route),
                        attempt_deadline: Some(deadline),
                        held_connection: Some(binding.connection_mut()),
                        load_policy,
                    },
                )
                .await;
                if let crate::native_message_dispatch::NativeMessageOutcome::Accepted(receipt) =
                    &mut response
                {
                    receipt.permission_differences = binding.permission_differences().to_vec();
                }
                match &response {
                    crate::native_message_dispatch::NativeMessageOutcome::Accepted(receipt)
                        if matches!(
                            receipt.acceptance,
                            NativeSendAcceptance::QueueAccepted { .. }
                        ) =>
                    {
                        self.holder.restore(*binding);
                        checkout.disarm();
                    }
                    crate::native_message_dispatch::NativeMessageOutcome::Failed(failure)
                        if matches!(
                            failure
                                .response
                                .pointer("/error/data/effects/submission")
                                .and_then(Value::as_str),
                            Some("notDispatched" | "rejected")
                        ) =>
                    {
                        self.holder.restore(*binding);
                        checkout.disarm();
                    }
                    _ => checkout.finish(),
                }
                (response, true)
            }
            HeldBindingCheckout::Busy => {
                effects.submission = SubmissionEffect::NotDispatched;
                effects.resume = PreparationEffect::NotRequested;
                sink.record(RouteEffectEvidence::CodexAppServer(effects))
                    .await?;
                return Ok(not_submitted(
                    "Held session is busy",
                    true,
                    collaboration_contracts::UnsentCause::Holdable {
                        otherwise: collaboration_contracts::NativeRefusalCode::HeldSessionBusy,
                    },
                ));
            }
            HeldBindingCheckout::Missing => {
                let response = crate::native_message_dispatch::dispatch_message(
                    crate::native_message_dispatch::NativeMessageRequest {
                        attempt_evidence: Some(
                            crate::message_effect_state::NativeMessageAttemptEvidence {
                                effects: &mut effects,
                                sink,
                            },
                        ),
                        params,
                        id: json!(request.attempt.as_str()),
                        service_id: &self.service_id,
                        backend: &self.backend,
                        endpoints: &endpoints,
                        approval_broker: self.approval_broker.as_ref(),
                        recorded_route: Some(&recorded_route),
                        attempt_deadline: Some(deadline),
                        held_connection: None,
                        load_policy,
                    },
                )
                .await;
                (response, false)
            }
        };
        let receipt = interpret_native_response(
            &request.target,
            response,
            held_idle_submission,
            &mut effects,
        );
        if sink
            .record(RouteEffectEvidence::CodexAppServer(effects))
            .await
            .is_err()
        {
            return Ok(collaboration_contracts::NativeDeliveryResult::unknown()
                .with_reachability(SessionReachability::CodexAppServer));
        }
        Ok(receipt)
    }

    async fn inspect_thread_presence(&self, target: &SessionRef) -> RoutePresence {
        if target.endpoint != self.backend.endpoint {
            return RoutePresence::NotMine;
        }
        let session_id = String::from(target.session_id.clone());
        if self.holder.contains(&session_id) {
            return RoutePresence::Running;
        }
        let Ok(admission) = self.backend.gate.acquire() else {
            return RoutePresence::Unreachable {
                reason: "native backend is unavailable".to_owned(),
            };
        };
        let Some(schemas) = admission.schemas() else {
            return RoutePresence::Unreachable {
                reason: "native thread/read is not supported".to_owned(),
            };
        };
        let endpoint_snapshot = self
            .endpoints
            .subscribe()
            .and_then(|subscription| subscription.snapshot());
        let Some(endpoint) = endpoint_snapshot.ok().and_then(|snapshot| {
            snapshot
                .endpoints
                .into_iter()
                .find(|entry| entry.endpoint == target.endpoint)
        }) else {
            return RoutePresence::Unreachable {
                reason: "Codex endpoint is not available".to_owned(),
            };
        };
        let schema_matches = endpoint.channels.iter().any(|channel| {
            matches!(
                channel,
                ChannelDescription::NativeCodex {
                    schema_digest: Some(digest),
                    generation: Some(generation),
                    ..
                } if generation == admission.generation()
                    && String::from(digest.clone()) == schemas.schema_digest()
            )
        });
        if !schema_matches {
            return RoutePresence::Unreachable {
                reason: "native thread/read schema is unavailable".to_owned(),
            };
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let retired = admission.retirement();
        let connection = tokio::time::timeout_at(deadline, async {
            tokio::select! {
                biased;
                _ = retired.cancelled() => Err(codex_native_integration::NativeConnectionError::Unavailable),
                result = NativeProtocolConnection::connect(admission.backend_path()) => result,
            }
        })
        .await
        .unwrap_or(Err(
            codex_native_integration::NativeConnectionError::Unavailable,
        ));
        let Ok(mut connection) = connection else {
            return RoutePresence::Unreachable {
                reason: "native backend is unavailable".to_owned(),
            };
        };
        let session_id = String::from(target.session_id.clone());
        match read_native_thread_status(&mut connection, &schemas, &session_id, deadline, &retired)
            .await
        {
            Ok(snapshot) if snapshot.status == NativeThreadStatus::NotLoaded => {
                RoutePresence::Wakeable
            }
            Ok(snapshot)
                if matches!(
                    snapshot.status,
                    NativeThreadStatus::Idle | NativeThreadStatus::Active
                ) =>
            {
                RoutePresence::Running
            }
            Ok(_) => RoutePresence::Unreachable {
                reason: "Codex thread status is unavailable".to_owned(),
            },
            Err(NativeThreadStatusReadError::Missing) => RoutePresence::Unreachable {
                reason: "Codex thread is missing".to_owned(),
            },
            Err(
                NativeThreadStatusReadError::Rejected { .. }
                | NativeThreadStatusReadError::LocalRejection(_)
                | NativeThreadStatusReadError::Unavailable,
            ) => RoutePresence::Unreachable {
                reason: "Codex thread could not be read".to_owned(),
            },
            Err(NativeThreadStatusReadError::InvalidResponse) => RoutePresence::Unreachable {
                reason: "Codex thread status is unavailable".to_owned(),
            },
        }
    }
}

impl SessionDeliveryRoute for CodexAppServerDeliveryRoute {
    fn scheduled_runs(
        &self,
    ) -> Option<std::sync::Arc<dyn collaboration_contracts::ScheduledRunRoute>> {
        Some(std::sync::Arc::new(
            crate::CodexAppServerScheduledRuns::new(
                self.backend.clone(),
                std::sync::Arc::clone(&self.holder),
            )
            .with_approval_broker(self.approval_broker.clone()),
        ))
    }

    fn reachability(&self) -> SessionReachability {
        SessionReachability::CodexAppServer
    }

    fn claim(
        &self,
        target: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::NativeRouteClaim> {
        let claim = if target.endpoint != self.backend.endpoint {
            collaboration_contracts::NativeRouteClaim::not_mine()
        } else if self.backend.gate.acquire().is_ok() {
            collaboration_contracts::NativeRouteClaim::holds()
        } else {
            collaboration_contracts::NativeRouteClaim::unavailable(
                RouteUnavailableReason {
                    reason: "Native backend unavailable".into(),
                    fix: "retry when the app-server is available".into(),
                },
                true,
                collaboration_contracts::UnsentCause::Holdable {
                    otherwise: collaboration_contracts::NativeRefusalCode::NativeUnavailable,
                },
            )
        };
        Box::pin(async move { Ok(claim) })
    }

    fn presence(&self, target: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
        let target = target.clone();
        Box::pin(async move { Ok(self.inspect_thread_presence(&target).await) })
    }

    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        sink: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            let push_id = &request.payload.push_id;
            let expected_link =
                RouterLink::new(MachineId::from(self.service_id.clone()), push_id.clone())
                    .to_string();
            let line_has_expected_link =
                prepared_line_has_expected_link(request.payload.line.as_str(), &expected_link);
            if request.correlation.as_str() != push_id.as_str() || !line_has_expected_link {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            self.deliver_native(
                NativeRouteDeliveryRequest {
                    target: request.target,
                    body: NativeMessageBody::PreparedPush(request.payload.line),
                    mode: request.mode,
                    load_policy: request.payload.load_policy,
                    precondition: request.precondition,
                    correlation: request.correlation,
                    attempt: request.attempt,
                },
                sink,
            )
            .await
        })
    }

    fn reconcile_attempt(
        &self,
        context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async move {
            Ok(crate::codex_queue_reconciliation::reconcile(&self.backend, context).await)
        })
    }
}

fn prepared_line_has_expected_link(line: &str, expected_link: &str) -> bool {
    if line.contains(['\n', '\r']) {
        return false;
    }
    let Some(prefix) = line.strip_suffix(expected_link) else {
        return false;
    };
    let Some((_, preview_suffix)) = prefix.split_once(" · \"") else {
        return prefix.strip_suffix(" · ").is_some();
    };
    if let Some(preview) = preview_suffix.strip_suffix("\" · ") {
        return preview_has_only_escaped_quotes(preview);
    }
    let Some(truncated) = preview_suffix.strip_suffix(") 📖 Load full message → ") else {
        return false;
    };
    let Some((preview, omitted_scalars)) = truncated.rsplit_once("…\" (+") else {
        return false;
    };
    // The formatter owns the count's source-body semantics. This route only admits
    // its exact positive decimal grammar before the exact service/push link.
    omitted_scalars
        .parse::<usize>()
        .is_ok_and(|count| count > 0 && count.to_string() == omitted_scalars)
        && preview_has_only_escaped_quotes(preview)
}

fn preview_has_only_escaped_quotes(preview: &str) -> bool {
    let mut escaped = false;
    for character in preview.chars() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return false;
        }
    }
    !escaped
}

#[path = "codex_app_server_delivery_result.rs"]
pub(crate) mod native_result;
use native_result::{interpret_native_response, not_submitted};
