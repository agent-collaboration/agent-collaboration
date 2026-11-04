//! Classify the full shared refusal vocabulary without serialized record bodies.
use crate::{
    fleet_reason_evidence::{EvidenceEntry, EvidenceRow},
    fleet_reason_fields::{ReasonFields, required},
    fleet_row_codec::corrupt,
};
use durable_steps::StoreError;
use message_board::Refusal;

pub(crate) fn encode(reason: &Refusal) -> Result<(ReasonFields, Vec<EvidenceEntry>), StoreError> {
    reason
        .validate_bounds()
        .map_err(|error| corrupt("reason", error))?;
    let mut evidence = Vec::new();
    let fields = match reason {
        Refusal::Thread(reason) => crate::fleet_thread_refusal::encode(reason)?,
        Refusal::Workstream(_) | Refusal::Task(_) | Refusal::Trail(_) | Refusal::Role(_) => {
            crate::fleet_refusal_groups::encode(reason, &mut evidence)?
        }
        Refusal::StaleRevision { current } => {
            let mut value = ReasonFields::tagged("root", "staleRevision");
            value.revision = Some(*current);
            value
        }
        Refusal::InvalidShape { field } => {
            let mut value = ReasonFields::tagged("root", "invalidShape");
            value.field = Some(field.clone());
            value
        }
        Refusal::NotFound(record) => {
            let mut value = ReasonFields::tagged("root", "notFound");
            value.record = Some(record.clone());
            value
        }
        Refusal::NameConflict { name, existing } => {
            let mut value = ReasonFields::tagged("root", "nameConflict");
            value.name = Some(name.clone());
            value.record = Some(existing.clone());
            value
        }
        Refusal::AlreadyExists(record) => {
            let mut value = ReasonFields::tagged("root", "alreadyExists");
            value.record = Some(record.clone());
            value
        }
        Refusal::TopLevelPostCooldown {
            retry_after_seconds,
        } => {
            let mut value = ReasonFields::tagged("root", "postCooldown");
            value.retry = Some(*retry_after_seconds);
            value
        }
        Refusal::Closed { subject } => {
            let mut value = ReasonFields::tagged("root", "closed");
            value.record = Some(subject.clone());
            value
        }
        Refusal::Archived => ReasonFields::tagged("root", "archived"),
        Refusal::ProjectRejected { .. } => ReasonFields::tagged("root", "projectRejected"),
    };
    Ok((fields, evidence))
}

pub(crate) fn decode(
    fields: ReasonFields,
    evidence: Vec<EvidenceRow>,
    id: &str,
) -> Result<Refusal, StoreError> {
    let has_evidence = matches!(
        (fields.group.as_str(), fields.kind.as_str()),
        ("task", "dependencyCycle") | ("trail", "subjectRequired")
    );
    if !has_evidence && !evidence.is_empty() {
        return Err(corrupt(
            "reason.evidence",
            "evidence outside its active refusal variant",
        ));
    }
    let reason = match fields.group.as_str() {
        "thread" => Refusal::Thread(Box::new(crate::fleet_thread_refusal::decode(fields)?)),
        "workstream" | "task" | "trail" | "role" => {
            crate::fleet_refusal_groups::decode(fields, evidence, id)?
        }
        "root" => match fields.kind.as_str() {
            "staleRevision" => {
                fields.active(&["revision"])?;
                Refusal::StaleRevision {
                    current: required(fields.revision, "reason.revision")?,
                }
            }
            "invalidShape" => {
                fields.active(&["field"])?;
                Refusal::InvalidShape {
                    field: required(fields.field, "reason.field")?,
                }
            }
            "notFound" => {
                fields.active(&["record"])?;
                Refusal::NotFound(required(fields.record, "reason.record")?)
            }
            "nameConflict" => {
                fields.active(&["name", "record"])?;
                Refusal::NameConflict {
                    name: required(fields.name, "reason.name")?,
                    existing: required(fields.record, "reason.record")?,
                }
            }
            "alreadyExists" => {
                fields.active(&["record"])?;
                Refusal::AlreadyExists(required(fields.record, "reason.record")?)
            }
            "postCooldown" => {
                fields.active(&["retry"])?;
                Refusal::TopLevelPostCooldown {
                    retry_after_seconds: required(fields.retry, "reason.retry")?,
                }
            }
            "closed" => {
                fields.active(&["record"])?;
                Refusal::Closed {
                    subject: required(fields.record, "reason.record")?,
                }
            }
            "archived" => {
                fields.active(&[])?;
                Refusal::Archived
            }
            _ => {
                return Err(corrupt(
                    "reason.kind",
                    "unknown root refusal or nested project wrapper",
                ));
            }
        },
        _ => return Err(corrupt("reason.group", "unknown refusal group")),
    };
    reason
        .validate_bounds()
        .map_err(|error| corrupt("reason", error))?;
    Ok(reason)
}
