//! Router-owned intent transitions, unavailable to public request deserialization.
use super::{ProjectAdmission, ProjectAdmissionError, request_gate::*};
use chrono::Utc;
use message_board::*;
use message_board_storage::ProjectWriteTransaction;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct IntentRequest {
    pub request_id: RequestId,
    pub project: ProjectId,
    pub observed_epoch: Option<EpochNumber>,
    pub resubmits: Option<RequestId>,
    pub operation: IntentOperation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum IntentOperation {
    Claim {
        intent: NotifyIntentId,
    },
    Refuse {
        intent: NotifyIntentId,
        detail: String,
    },
}

pub enum IntentReply {
    Recorded {
        write: WriteReply,
        claim: Option<IntentClaim>,
    },
    Deferred {
        recipient: ResolvedRecipient,
        replacements: Vec<NotifyIntentId>,
    },
}

struct IntentTransition {
    decision: RequestDecision,
    claim: Option<IntentClaim>,
}

impl ProjectAdmission {
    /// Trusted C5 port; public board requests cannot select a Router actor.
    pub async fn claim_intent(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
    ) -> Result<IntentReply, ProjectAdmissionError> {
        self.submit_intent(&IntentRequest {
            request_id: RequestId::generate(),
            project: project.clone(),
            observed_epoch: None,
            resubmits: None,
            operation: IntentOperation::Claim {
                intent: intent.clone(),
            },
        })
        .await
    }

    /// Report only a definitive push-port refusal for an already claimed session.
    pub async fn refuse_intent(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        detail: String,
    ) -> Result<IntentReply, ProjectAdmissionError> {
        self.submit_intent(&IntentRequest {
            request_id: RequestId::generate(),
            project: project.clone(),
            observed_epoch: None,
            resubmits: None,
            operation: IntentOperation::Refuse {
                intent: intent.clone(),
                detail,
            },
        })
        .await
    }

    pub(crate) async fn submit_intent(
        &self,
        request: &IntentRequest,
    ) -> Result<IntentReply, ProjectAdmissionError> {
        let semantic = serde_json::to_vec(&(
            &request.project,
            RouterComponent::SubscriptionDelivery,
            &request.resubmits,
            ProtocolVersion::CURRENT,
            &request.operation,
        ))
        .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        let hash = CanonicalHash::try_from(format!("{:x}", Sha256::digest(semantic)))
            .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        let ready = self
            .begin_request(RequestGateInput {
                project: &request.project,
                request: &request.request_id,
                observed_epoch: request.observed_epoch,
                hash: &hash,
            })
            .await?;
        let (result, claim) = match ready {
            RequestGate::Reply(reply) => (reply, None),
            RequestGate::Ready(ready) => {
                let ReadyRequest {
                    mut write,
                    epoch,
                    high_water,
                } = *ready;
                let project_ref = request.project.clone().into();
                let Some(Record::Project(project)) = write.record(&project_ref).await? else {
                    return Err(ProjectAdmissionError::InvalidRecord(
                        "project state is missing".into(),
                    ));
                };
                let envelope = RecordEnvelope {
                    id: request.request_id.clone().into(),
                    scope: project.envelope.scope,
                    actor: RecordedActor {
                        actor: ActorKind::Router {
                            component: RouterComponent::SubscriptionDelivery,
                        },
                        installation: self.installation.clone(),
                        machine: self.machine.clone(),
                    },
                    request: Some(request.request_id.clone()),
                    position: RecordPosition::Project(high_water),
                    router_time: Utc::now(),
                };
                let IntentTransition { decision, claim } =
                    intent_transition(&mut write, &request.operation, &envelope).await?;
                if matches!(
                    decision,
                    RequestDecision::Accepted {
                        change: ChangeOutcome::AlreadySo,
                        ..
                    }
                ) && let Some(IntentClaim::Deferred {
                    recipient,
                    replacements,
                }) = claim
                {
                    write.rollback().await?;
                    return Ok(IntentReply::Deferred {
                        recipient,
                        replacements,
                    });
                }
                write
                    .append_record(Record::Request(RequestLedgerEntry {
                        envelope,
                        request_id: request.request_id.clone(),
                        canonical_hash: hash,
                        observed_epoch: request.observed_epoch,
                        accepted_in: Some(epoch),
                        decision: decision.clone(),
                        resubmits: request.resubmits.clone(),
                    }))
                    .await?;
                write.commit().await?;
                (RequestResult::Decided(decision), claim)
            }
        };
        Ok(IntentReply::Recorded {
            write: WriteReply {
                result,
                reader: None,
            },
            claim,
        })
    }
}

async fn intent_transition(
    write: &mut ProjectWriteTransaction,
    operation: &IntentOperation,
    envelope: &RecordEnvelope,
) -> Result<IntentTransition, ProjectAdmissionError> {
    let intent_id = match operation {
        IntentOperation::Claim { intent } | IntentOperation::Refuse { intent, .. } => intent,
    };
    let Some(Record::NotifyIntent(original)) = write.record(&intent_id.clone().into()).await?
    else {
        return Ok(IntentTransition {
            decision: RequestDecision::Rejected {
                reasons: vec![Refusal::NotFound(intent_id.clone().into())],
            },
            claim: None,
        });
    };
    let mut intent = original.clone();
    let mut records = Vec::new();
    let mut claim = None;
    match operation {
        IntentOperation::Claim { .. } if matches!(intent.state(), IntentState::Emitted {}) => {
            let resolved = super::intent_re_resolution::re_resolve(write, intent, envelope).await?;
            intent = resolved.original;
            records = resolved.changed;
            let replacements = resolved.replacements;
            if matches!(intent.state(), IntentState::Superseded {}) {
                claim = Some(IntentClaim::Superseded { replacements });
            } else {
                let IntentRecipient::Resolved(recipient) = intent.recipient() else {
                    return Err(ProjectAdmissionError::InvalidRecord(
                        "emitted intent has no recipient".into(),
                    ));
                };
                let recipient = recipient.clone();
                if matches!(recipient.actor.declared_identity(), Some(Identity::Session { session }) if session.endpoint.service_id == envelope.actor.installation)
                {
                    intent
                        .set_state(IntentState::HandedOff {})
                        .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
                    claim = Some(IntentClaim::Claimed {
                        recipient,
                        replacements,
                    });
                } else {
                    claim = Some(IntentClaim::Deferred {
                        recipient,
                        replacements,
                    });
                }
            }
        }
        IntentOperation::Refuse { detail, .. }
            if matches!(intent.state(), IntentState::HandedOff {}) =>
        {
            intent
                .set_state(IntentState::Refused {
                    reason: IntentRefusal::Delivery {
                        detail: detail.clone(),
                    },
                })
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        }
        _ => {}
    }
    if intent != original {
        records.insert(0, intent);
    }
    let mut references = Vec::new();
    let mut positions = Vec::new();
    for mut record in records {
        record.envelope = RecordEnvelope {
            id: record.intent_id.clone().into(),
            ..envelope.clone()
        };
        references.push(record.intent_id.clone().into());
        positions.push(RecordPosition::Project(
            write.append_record(Record::NotifyIntent(record)).await?,
        ));
    }
    Ok(IntentTransition {
        decision: RequestDecision::Accepted {
            change: if references.is_empty() {
                ChangeOutcome::AlreadySo
            } else {
                ChangeOutcome::Changed
            },
            records: references,
            positions,
        },
        claim,
    })
}
