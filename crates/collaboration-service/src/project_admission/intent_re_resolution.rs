//! NI-2 compares a role's current holder set against the cause's stable actor intents.
use super::ProjectAdmissionError;
use message_board::*;
use message_board_storage::ProjectWriteTransaction;

pub(super) struct IntentResolution {
    pub original: NotifyIntent,
    pub changed: Vec<NotifyIntent>,
    pub replacements: Vec<NotifyIntentId>,
}

pub(super) async fn re_resolve(
    write: &mut ProjectWriteTransaction,
    original: NotifyIntent,
    envelope: &RecordEnvelope,
) -> Result<IntentResolution, ProjectAdmissionError> {
    let mut result = IntentResolution {
        original,
        changed: Vec::new(),
        replacements: Vec::new(),
    };
    let IntentRecipient::Resolved(recipient) = result.original.recipient() else {
        return Ok(result);
    };
    let Some(provenance) = recipient.via_role.clone() else {
        return Ok(result);
    };
    let revision = write.assignment_revision(&provenance.subject).await?;
    if revision == provenance.assignment_revision {
        return Ok(result);
    }
    let selector = Assignment::new(provenance.subject.clone(), provenance.role)
        .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
    let resolution = crate::board_views::resolve(write, &selector).await?;
    let empty = matches!(resolution, RoleResolution::NoHolder { .. });
    let actors = match resolution {
        RoleResolution::Holders { actors, .. } => actors,
        RoleResolution::NoHolder { .. } => Default::default(),
    };
    let existing = write.cause_intents(&result.original.cause).await?;
    let already_empty = existing.iter().any(|intent| matches!(intent.recipient(), IntentRecipient::NoHolder { subject, role, .. } if *subject == provenance.subject && *role == provenance.role));
    // Once this cause reached an empty set, it is terminal for that selector.
    let actors = if already_empty {
        Default::default()
    } else {
        actors
    };
    for intent in &existing {
        let IntentRecipient::Resolved(recipient) = intent.recipient() else {
            continue;
        };
        if !matches!(intent.state(), IntentState::Emitted {})
            || !recipient.via_role.as_ref().is_some_and(|role| {
                role.subject == provenance.subject && role.role == provenance.role
            })
        {
            continue;
        }
        let retained = actors
            .iter()
            .any(|actor| actor.declared_identity() == recipient.actor.declared_identity());
        let mut props = NotifyIntentProps::from(intent.clone());
        if retained {
            if let IntentRecipient::Resolved(recipient) = &mut props.recipient
                && let Some(role) = &mut recipient.via_role
            {
                role.assignment_revision = revision;
            }
        } else {
            props.state = IntentState::Superseded {};
        }
        let updated = NotifyIntent::new(props).map_err(invalid)?;
        if updated.intent_id == result.original.intent_id {
            result.original = updated;
        } else if updated != *intent {
            result.changed.push(updated);
        }
    }
    for actor in actors {
        let previous = existing.iter().find(|intent| matches!(intent.recipient(), IntentRecipient::Resolved(recipient) if recipient.actor.declared_identity() == actor.declared_identity()));
        let recipient = IntentRecipient::Resolved(ResolvedRecipient {
            actor,
            via_role: Some(RoleRecipient {
                assignment_revision: revision,
                ..provenance.clone()
            }),
        });
        let intent = match previous {
            Some(previous) if matches!(previous.state(), IntentState::Superseded {}) => {
                let mut props = NotifyIntentProps::from(previous.clone());
                props.recipient = recipient;
                props.state = IntentState::Emitted {};
                NotifyIntent::new(props).map_err(invalid)?
            }
            Some(_) => continue,
            None => {
                let id = NotifyIntentId::generate();
                NotifyIntent::new(NotifyIntentProps {
                    envelope: RecordEnvelope {
                        id: id.clone().into(),
                        ..envelope.clone()
                    },
                    intent_id: id,
                    cause: result.original.cause.clone(),
                    recipient,
                    state: IntentState::Emitted {},
                })
                .map_err(invalid)?
            }
        };
        result.replacements.push(intent.intent_id.clone());
        result.changed.push(intent);
    }
    if matches!(result.original.state(), IntentState::Superseded {}) && !already_empty && empty {
        let id = NotifyIntentId::generate();
        result.changed.push(
            NotifyIntent::new(NotifyIntentProps {
                envelope: RecordEnvelope {
                    id: id.clone().into(),
                    ..envelope.clone()
                },
                intent_id: id,
                cause: result.original.cause.clone(),
                recipient: IntentRecipient::NoHolder {
                    subject: provenance.subject,
                    role: provenance.role,
                    assignment_revision: revision,
                },
                state: IntentState::Refused {
                    reason: IntentRefusal::NoHolder {},
                },
            })
            .map_err(invalid)?,
        );
    }
    Ok(result)
}
fn invalid(error: TextValidationError) -> ProjectAdmissionError {
    ProjectAdmissionError::InvalidRecord(error.to_string())
}
