//! Claude Code peer reachability and write-only message delivery.
use crate::live_session_ownership_check::{LiveSessionOwnership, LiveSessionOwnershipCheck};
use agent_automation::{
    ClaudeCodePeerEffectEvidence, PeerSessionReference, PeerWriteEffect, RouteEffectEvidence,
};
use claude_code_peer_messaging::{
    ClaudeCodePeerSocket, ClaudeCodeSessionRegistry, PeerRegistryError, PeerSessionLookup,
    PeerSessionRecord, PeerSocketUnsentCause, PeerSocketWriteOutcome,
};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_contracts::NativeRefusalCode;
use collaboration_contracts::NativeRouteClaim;
use collaboration_contracts::RoutePresence;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::UnsentCause;
use collaboration_protocol::{
    CodexGeneration, DeliveryClientReceipt, DeliveryNextAction, DeliveryPeerClaim,
    DeliveryRejection, DeliveryRejectionReason, EndpointRef, MessageDelivery, SessionReachability,
    SessionRef,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct ClaudeCodePeerDeliveryRoute {
    endpoint: EndpointRef,
    registry: Arc<ClaudeCodeSessionRegistry>,
    socket: Arc<ClaudeCodePeerSocket>,
}

enum PeerDeliveryPreparation {
    Ready(PeerSessionRecord),
    Finished(Box<NativeDeliveryResult>),
}

impl PeerDeliveryPreparation {
    fn finished(result: NativeDeliveryResult) -> Self {
        Self::Finished(Box::new(result))
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PeerLookupFailure {
    #[error("{0}")]
    Registry(#[from] PeerRegistryError),
    #[error("Claude Code registry lookup task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

const AMBIGUOUS_PEER_CLAIM_REASON: &str =
    "multiple live Claude Code registry records claim this session";

impl ClaudeCodePeerDeliveryRoute {
    #[must_use]
    pub fn new(
        endpoint: EndpointRef,
        registry: Arc<ClaudeCodeSessionRegistry>,
        socket: Arc<ClaudeCodePeerSocket>,
    ) -> Self {
        Self {
            endpoint,
            registry,
            socket,
        }
    }

    pub(crate) fn serves(&self, target: &SessionRef) -> bool {
        target.endpoint == self.endpoint
    }

    pub(crate) async fn lookup(
        &self,
        target: &SessionRef,
    ) -> Result<PeerSessionLookup, PeerLookupFailure> {
        let registry = Arc::clone(&self.registry);
        let session_id = target.session_id.clone();
        Ok(tokio::task::spawn_blocking(move || registry.lookup(&session_id)).await??)
    }

    pub(crate) fn evidence(
        peer: &PeerSessionRecord,
        write: PeerWriteEffect,
    ) -> Result<RouteEffectEvidence<SessionRef, CodexGeneration>, DeliveryContractError> {
        let session_id = PeerSessionReference::try_from(String::from(peer.session_id.clone()))
            .map_err(|_| DeliveryContractError::InvalidEvidence)?;
        Ok(RouteEffectEvidence::ClaudeCodePeer(
            ClaudeCodePeerEffectEvidence {
                session_id,
                process_id: peer.process_id,
                write,
            },
        ))
    }

    pub(crate) async fn write_peer_message(
        &self,
        peer: &PeerSessionRecord,
        prepared_line: &str,
    ) -> PeerSocketWriteOutcome {
        self.socket.write_user_message(peer, prepared_line).await
    }

    async fn prepare_peer_delivery(
        &self,
        target: &SessionRef,
        precondition: &DeliveryPrecondition,
        mode: MessageDelivery,
    ) -> Result<PeerDeliveryPreparation, DeliveryContractError> {
        if !self.serves(target) {
            return Ok(PeerDeliveryPreparation::finished(peer_rejection(
                DeliveryRejectionReason::NoRoute,
                "Claude Code peer route does not serve this endpoint",
                UnsentCause::RecipientUnknown,
            )));
        }
        if matches!(
            precondition,
            DeliveryPrecondition::EndpointGeneration { .. }
        ) {
            return Ok(PeerDeliveryPreparation::finished(peer_rejection(
                DeliveryRejectionReason::StaleGeneration,
                "Claude Code peer sessions have no endpoint generation",
                UnsentCause::for_rejection(DeliveryRejectionReason::StaleGeneration),
            )));
        }
        if mode == MessageDelivery::Queue {
            return Ok(PeerDeliveryPreparation::finished(peer_rejection(
                DeliveryRejectionReason::QueueUnsupported,
                "Queue delivery isn't supported for Claude Code terminals",
                UnsentCause::for_rejection(DeliveryRejectionReason::QueueUnsupported),
            )));
        }
        Ok(match self.lookup(target).await {
            Ok(PeerSessionLookup::Absent) => PeerDeliveryPreparation::finished(
                NativeDeliveryResult::not_submitted(
                    "Claude Code session is no longer live",
                    true,
                    UnsentCause::Holdable {
                        otherwise: NativeRefusalCode::PeerUnavailable,
                    },
                )
                .with_reachability(SessionReachability::ClaudeCodePeer),
            ),
            Ok(PeerSessionLookup::LiveUnsupported { reason }) => {
                PeerDeliveryPreparation::finished(peer_rejection(
                    DeliveryRejectionReason::LiveElsewhere,
                    &reason,
                    UnsentCause::for_rejection(DeliveryRejectionReason::LiveElsewhere),
                ))
            }
            Err(error) => PeerDeliveryPreparation::finished(peer_rejection(
                DeliveryRejectionReason::LiveElsewhere,
                &error.to_string(),
                UnsentCause::ProcessingFailure,
            )),
            Ok(PeerSessionLookup::Ambiguous { claims }) => PeerDeliveryPreparation::finished(
                NativeDeliveryResult::rejected(
                    peer_claim_rejection(claims),
                    UnsentCause::Refused(NativeRefusalCode::Rejection(
                        DeliveryRejectionReason::LiveElsewhere,
                    )),
                )
                .with_reachability(SessionReachability::ClaudeCodePeer),
            ),
            Ok(PeerSessionLookup::Writable(peer)) => PeerDeliveryPreparation::Ready(peer),
        })
    }

    async fn write_peer_delivery(
        &self,
        peer: &PeerSessionRecord,
        text: &str,
        evidence: &dyn AttemptEvidenceSink,
    ) -> Result<NativeDeliveryResult, DeliveryContractError> {
        evidence
            .record(Self::evidence(peer, PeerWriteEffect::Dispatching)?)
            .await?;
        match self.socket.write_user_message(peer, text).await {
            PeerSocketWriteOutcome::Written => {
                if evidence
                    .record(Self::evidence(peer, PeerWriteEffect::Written)?)
                    .await
                    .is_err()
                {
                    return Ok(peer_observed(NativeObservedOutcome::Unknown, None));
                }
                Ok(peer_observed(
                    NativeObservedOutcome::PeerMessageWritten,
                    Some(DeliveryClientReceipt::ClaudeCodePeer),
                ))
            }
            PeerSocketWriteOutcome::NotSubmitted { reason, cause } => {
                if evidence
                    .record(Self::evidence(peer, PeerWriteEffect::NotDispatched)?)
                    .await
                    .is_err()
                {
                    return Ok(peer_observed(NativeObservedOutcome::Unknown, None));
                }
                Ok(NativeDeliveryResult::not_submitted(
                    reason,
                    true,
                    UnsentCause::Holdable {
                        otherwise: peer_socket_code(cause),
                    },
                )
                .with_reachability(SessionReachability::ClaudeCodePeer))
            }
            PeerSocketWriteOutcome::Unknown { .. } => {
                if evidence
                    .record(Self::evidence(peer, PeerWriteEffect::Unknown)?)
                    .await
                    .is_err()
                {
                    return Ok(peer_observed(NativeObservedOutcome::Unknown, None));
                }
                Ok(peer_observed(NativeObservedOutcome::Unknown, None))
            }
        }
    }
}

fn peer_observed(
    outcome: NativeObservedOutcome,
    client: Option<DeliveryClientReceipt>,
) -> NativeDeliveryResult {
    NativeDeliveryResult::observed(outcome, Some(SessionReachability::ClaudeCodePeer), client)
}
fn peer_rejection(
    reason: DeliveryRejectionReason,
    detail: &str,
    cause: UnsentCause,
) -> NativeDeliveryResult {
    let next_action = if reason == DeliveryRejectionReason::LiveElsewhere {
        DeliveryNextAction::InspectTarget
    } else {
        DeliveryNextAction::CorrectRequest
    };
    NativeDeliveryResult::rejected(
        DeliveryRejection {
            reason,
            next_action,
            client_code: None,
            detail: Some(detail.to_owned()),
            claims: None,
            native_failure: None,
        },
        cause,
    )
    .with_reachability(SessionReachability::ClaudeCodePeer)
}
fn peer_socket_code(cause: PeerSocketUnsentCause) -> NativeRefusalCode {
    match cause {
        PeerSocketUnsentCause::AuthenticationUnavailable => {
            NativeRefusalCode::PeerAuthenticationUnavailable
        }
        PeerSocketUnsentCause::EncodingFailed => NativeRefusalCode::PeerEncodingFailed,
        PeerSocketUnsentCause::FrameTooLarge => NativeRefusalCode::PeerFrameTooLarge,
        PeerSocketUnsentCause::ConnectFailed => NativeRefusalCode::PeerConnectFailed,
        PeerSocketUnsentCause::WriteNotStarted => NativeRefusalCode::PeerWriteNotStarted,
    }
}

fn peer_claim_rejection(claims: Vec<claude_code_peer_messaging::PeerClaim>) -> DeliveryRejection {
    let claim_count = claims.len();
    let claims = claims
        .into_iter()
        .map(|claim| DeliveryPeerClaim {
            pid: claim.pid,
            name: claim.name,
            cwd: claim.cwd.map(|cwd| cwd.to_string_lossy().into_owned()),
        })
        .collect();
    DeliveryRejection {
        reason: DeliveryRejectionReason::LiveElsewhere,
        next_action: DeliveryNextAction::InspectTarget,
        client_code: None,
        detail: Some(format!(
            "this Claude session is claimed by {claim_count} live terminals"
        )),
        claims: Some(claims),
        native_failure: None,
    }
}

impl LiveSessionOwnershipCheck for ClaudeCodePeerDeliveryRoute {
    fn check<'a>(&'a self, target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async move {
            if !self.serves(target) {
                return Ok(LiveSessionOwnership::NotLive);
            }
            Ok(match self.lookup(target).await {
                Ok(PeerSessionLookup::Absent) => LiveSessionOwnership::NotLive,
                Ok(PeerSessionLookup::Writable(_)) => LiveSessionOwnership::LiveWritable,
                Err(_)
                | Ok(
                    PeerSessionLookup::LiveUnsupported { .. } | PeerSessionLookup::Ambiguous { .. },
                ) => LiveSessionOwnership::LiveUnsupported,
            })
        })
    }
}

impl SessionDeliveryRoute for ClaudeCodePeerDeliveryRoute {
    fn scheduled_runs(&self) -> Option<Arc<dyn collaboration_contracts::ScheduledRunRoute>> {
        Some(Arc::new(self.clone()))
    }
    fn reachability(&self) -> SessionReachability {
        SessionReachability::ClaudeCodePeer
    }

    fn supports_delivery_mode(&self, target: &SessionRef, mode: MessageDelivery) -> bool {
        !self.serves(target) || mode != MessageDelivery::Queue
    }

    fn claim(&self, target: &SessionRef) -> DeliveryFuture<'_, NativeRouteClaim> {
        let target = target.clone();
        Box::pin(async move {
            if !self.serves(&target) {
                return Ok(NativeRouteClaim::not_mine());
            }
            Ok(match self.lookup(&target).await {
                Ok(PeerSessionLookup::Absent) => NativeRouteClaim::not_mine(),
                Ok(PeerSessionLookup::Writable(_)) => NativeRouteClaim::holds(),
                Ok(PeerSessionLookup::LiveUnsupported { reason }) => {
                    NativeRouteClaim::live_elsewhere(
                        false,
                        Some(reason),
                        UnsentCause::for_rejection(DeliveryRejectionReason::LiveElsewhere),
                    )
                }
                Err(error) => NativeRouteClaim::live_elsewhere(
                    false,
                    Some(error.to_string()),
                    UnsentCause::ProcessingFailure,
                ),
                Ok(PeerSessionLookup::Ambiguous { claims }) => NativeRouteClaim::rejected(
                    peer_claim_rejection(claims),
                    UnsentCause::for_rejection(DeliveryRejectionReason::LiveElsewhere),
                ),
            })
        })
    }

    fn presence(&self, target: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
        let target = target.clone();
        Box::pin(async move {
            if !self.serves(&target) {
                return Ok(RoutePresence::NotMine);
            }
            Ok(match self.lookup(&target).await {
                Ok(PeerSessionLookup::Absent) => RoutePresence::NotMine,
                Ok(PeerSessionLookup::Writable(_)) => RoutePresence::Running,
                Ok(PeerSessionLookup::LiveUnsupported { reason }) => RoutePresence::LiveElsewhere {
                    detail: Some(reason),
                },
                Err(error) => RoutePresence::LiveElsewhere {
                    detail: Some(error.to_string()),
                },
                Ok(PeerSessionLookup::Ambiguous { .. }) => RoutePresence::LiveElsewhere {
                    detail: Some(AMBIGUOUS_PEER_CLAIM_REASON.to_owned()),
                },
            })
        })
    }

    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, NativeDeliveryResult> {
        Box::pin(async move {
            if request.correlation.as_str() != request.payload.push_id.as_str() {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            let peer = match self
                .prepare_peer_delivery(&request.target, &request.precondition, request.mode)
                .await?
            {
                PeerDeliveryPreparation::Ready(peer) => peer,
                PeerDeliveryPreparation::Finished(receipt) => return Ok(*receipt),
            };
            self.write_peer_delivery(&peer, request.payload.line.as_str(), evidence)
                .await
        })
    }

    fn reconcile_attempt(
        &self,
        context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async move {
            let RouteEffectEvidence::ClaudeCodePeer(peer) = context.recorded else {
                return Err(DeliveryContractError::InvalidEvidence);
            };
            if !self.serves(&context.target)
                || peer.session_id.as_str() != String::from(context.target.session_id).as_str()
            {
                return Err(DeliveryContractError::InvalidEvidence);
            }
            Ok(match peer.write {
                PeerWriteEffect::NotDispatched => AttemptReconciliation::KnownNotSubmitted,
                PeerWriteEffect::Written => AttemptReconciliation::Accepted(Box::new(
                    peer_observed(
                        NativeObservedOutcome::PeerMessageWritten,
                        Some(DeliveryClientReceipt::ClaudeCodePeer),
                    )
                    .into_receipt(),
                )),
                PeerWriteEffect::Dispatching | PeerWriteEffect::Unknown => {
                    AttemptReconciliation::StillUnknown
                }
            })
        })
    }
}
