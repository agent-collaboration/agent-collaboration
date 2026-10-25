//! Router-owned intent transitions, unavailable to public request deserialization.
use super::intent_handoff::{IntentHandoffInput, freeze_intent_handoff, record_intent_handoff};
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
        handoff: Option<IntentHandoffInput>,
    },
    RecipientUnknown {
        intent: NotifyIntentId,
        service: ServiceId,
    },
}

pub enum IntentReply {
    Recorded {
        transition: Option<FeedCursor>,
        write: Box<WriteReply>,
        claim: Option<IntentClaim>,
    },
    Deferred {
        recipient: ResolvedRecipient,
        replacements: Vec<NotifyIntentId>,
    },
}

struct IntentTransition {
    decision: StoredDecision,
    claim: Option<IntentClaim>,
    transition: Option<FeedCursor>,
}

impl ProjectAdmission {
    /// The publisher supplies an enrollment pair obtained outside the project lock.
    pub async fn claim_intent(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        handoff: Option<IntentHandoffInput>,
    ) -> Result<IntentReply, ProjectAdmissionError> {
        self.submit_intent(&IntentRequest {
            request_id: RequestId::generate(),
            project: project.clone(),
            operation: IntentOperation::Claim {
                intent: intent.clone(),
                handoff,
            },
        })
        .await
    }

    pub(crate) async fn refuse_unknown_intent_recipient(
        &self,
        project: &ProjectId,
        intent: &NotifyIntentId,
        service: ServiceId,
    ) -> Result<IntentReply, ProjectAdmissionError> {
        self.submit_intent(&IntentRequest {
            request_id: RequestId::generate(),
            project: project.clone(),
            operation: IntentOperation::RecipientUnknown {
                intent: intent.clone(),
                service,
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
            ProtocolVersion::CURRENT,
            &request.operation,
        ))
        .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        let hash = CanonicalHash::try_from(format!("{:x}", Sha256::digest(semantic)))
            .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        let ready = self
            .begin_request(
                RequestGateInput {
                    project: &request.project,
                    request: &request.request_id,
                    hash: &hash,
                },
                self.record_origin(),
            )
            .await?;
        let (result, claim, transition) = match ready {
            RequestGate::Reply(reply) => (reply, None, None),
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
                    actor: RecordedActor::Router {
                        component: RouterComponent::SubscriptionDelivery,
                        installation: self.installation.clone(),
                    },
                    request: Some(request.request_id.clone()),
                    position: write.record_placement(high_water),
                    time: RecordTime::Router(Utc::now()),
                };
                let IntentTransition {
                    decision,
                    claim,
                    transition,
                } = intent_transition(&mut write, &request.operation, &envelope).await?;
                if matches!(
                    decision,
                    StoredDecision::Accepted {
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
                let ledger = RequestLedgerEntry {
                    envelope,
                    request_id: request.request_id.clone(),
                    canonical_hash: hash,
                    accepted_in: Some(epoch),
                    decision: decision.clone(),
                };
                let reply = write.request_decision(&ledger).await?;
                write.append_record(Record::Request(ledger)).await?;
                write.commit().await?;
                self.registry.notify_activity(&request.project);
                (RequestResult::Decided(reply), claim, transition)
            }
        };
        Ok(IntentReply::Recorded {
            transition,
            write: Box::new(WriteReply {
                result,
                reader: None,
            }),
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
        IntentOperation::Claim { intent, .. }
        | IntentOperation::RecipientUnknown { intent, .. } => intent,
    };
    let Some(Record::NotifyIntent(original)) = write.record(&intent_id.clone().into()).await?
    else {
        return Ok(IntentTransition {
            decision: StoredDecision::Rejected {
                reasons: vec![Refusal::NotFound(intent_id.clone().into())],
            },
            claim: None,
            transition: None,
        });
    };
    let mut intent = original.clone();
    let mut records = Vec::new();
    let mut claim = None;
    match operation {
        IntentOperation::Claim { .. } | IntentOperation::RecipientUnknown { .. }
            if matches!(intent.state(), IntentState::Emitted {}) =>
        {
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
                match (operation, recipient.actor.declared_identity()) {
                    (
                        IntentOperation::Claim {
                            handoff: Some(input),
                            ..
                        },
                        Some(identity @ Identity::Session { .. }),
                    ) => {
                        let frozen =
                            freeze_intent_handoff(write, &intent, &identity, input).await?;
                        intent
                            .set_state(IntentState::HandedOff { item: frozen.id()? })
                            .map_err(|error| {
                                ProjectAdmissionError::InvalidRecord(error.to_string())
                            })?;
                        record_intent_handoff(write, &intent, &frozen).await?;
                        claim = Some(IntentClaim::Claimed {
                            recipient,
                            replacements,
                        });
                    }
                    (
                        IntentOperation::RecipientUnknown { service, .. },
                        Some(Identity::Session { session }),
                    ) => {
                        if session.endpoint.service_id != *service {
                            return Err(ProjectAdmissionError::InvalidRecord(
                                "unknown intent recipient enrollment service".to_owned(),
                            ));
                        }
                        intent
                            .set_state(IntentState::Refused {
                                reason: IntentRefusal::RecipientUnknown {},
                            })
                            .map_err(|error| {
                                ProjectAdmissionError::InvalidRecord(error.to_string())
                            })?;
                    }
                    _ => {
                        claim = Some(IntentClaim::Deferred {
                            recipient,
                            replacements,
                        })
                    }
                }
            }
        }
        _ => {}
    }
    if intent != original {
        records.insert(0, intent);
    }
    let mut references = Vec::new();
    let mut positions = Vec::new();
    let mut transition = None;
    for mut record in records {
        let original_intent = record.intent_id == *intent_id;
        record.envelope = RecordEnvelope {
            id: record.intent_id.clone().into(),
            ..envelope.clone()
        };
        references.push(record.intent_id.clone().into());
        let cursor = write.append_record(Record::NotifyIntent(record)).await?;
        if original_intent {
            transition = Some(cursor);
        }
        positions.push(write.record_placement(cursor));
    }
    Ok(IntentTransition {
        decision: StoredDecision::accepted(
            references.iter().cloned().zip(positions).collect(),
            if references.is_empty() {
                ChangeOutcome::AlreadySo
            } else {
                ChangeOutcome::Changed
            },
        ),
        claim,
        transition,
    })
}
