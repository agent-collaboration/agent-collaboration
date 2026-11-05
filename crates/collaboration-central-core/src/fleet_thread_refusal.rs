//! Conversation-seat refusals retain exactly their own optional evidence rules.
use crate::{
    fleet_reason_fields::{ReasonFields, required},
    fleet_row_codec::corrupt,
};
use durable_steps::StoreError;
use message_board::ThreadRefusal;

pub(crate) fn encode(reason: &ThreadRefusal) -> Result<ReasonFields, StoreError> {
    reason
        .validate()
        .map_err(|error| corrupt("reason.thread", error))?;
    let (kind, thread, actor) = match reason {
        ThreadRefusal::ParticipantRequired { thread_id, actor } => {
            ("participantRequired", thread_id, actor)
        }
        ThreadRefusal::OrchestratorRequired {
            thread_id, actor, ..
        } => ("orchestratorRequired", thread_id, actor),
        ThreadRefusal::OrchestratorAlreadyExists {
            thread_id, actor, ..
        } => ("orchestratorAlreadyExists", thread_id, actor),
        ThreadRefusal::ImplementerAlreadyExists {
            thread_id, actor, ..
        } => ("implementerAlreadyExists", thread_id, actor),
        ThreadRefusal::OrchestratorHandoverRequired { thread_id, actor } => {
            ("orchestratorHandoverRequired", thread_id, actor)
        }
        ThreadRefusal::HandoverTargetNotParticipant {
            thread_id, actor, ..
        } => ("handoverTargetNotParticipant", thread_id, actor),
        ThreadRefusal::StaleOrchestrator {
            thread_id, actor, ..
        } => ("staleOrchestrator", thread_id, actor),
        ThreadRefusal::StaleImplementer {
            thread_id, actor, ..
        } => ("staleImplementer", thread_id, actor),
        ThreadRefusal::SelfReplace { thread_id, actor } => ("selfReplace", thread_id, actor),
        ThreadRefusal::OrchestratorRoleChange { thread_id, actor } => {
            ("orchestratorRoleChange", thread_id, actor)
        }
    };
    let mut fields = ReasonFields::tagged("thread", kind);
    fields.thread = Some(thread.clone());
    fields.actor = Some(actor.clone());
    match reason {
        ThreadRefusal::OrchestratorRequired {
            holder, last_seen, ..
        } => {
            fields.holder = holder.clone();
            fields.last_seen = *last_seen;
        }
        ThreadRefusal::OrchestratorAlreadyExists {
            holder, last_seen, ..
        }
        | ThreadRefusal::ImplementerAlreadyExists {
            holder, last_seen, ..
        } => {
            fields.holder = Some(holder.clone());
            fields.last_seen = Some(*last_seen);
        }
        ThreadRefusal::HandoverTargetNotParticipant { target, .. } => {
            fields.target = Some(target.clone());
        }
        ThreadRefusal::StaleOrchestrator {
            holder,
            last_seen,
            named_holder,
            ..
        }
        | ThreadRefusal::StaleImplementer {
            holder,
            last_seen,
            named_holder,
            ..
        } => {
            fields.holder = holder.clone();
            fields.last_seen = *last_seen;
            fields.named_holder = Some(named_holder.clone());
        }
        _ => {}
    }
    Ok(fields)
}

pub(crate) fn decode(mut fields: ReasonFields) -> Result<ThreadRefusal, StoreError> {
    let thread_id = required(fields.thread.take(), "reason.thread_id")?;
    let actor = required(fields.actor.take(), "reason.actor")?;
    let reason = match fields.kind.as_str() {
        "participantRequired" => {
            fields.active(&[])?;
            ThreadRefusal::ParticipantRequired { thread_id, actor }
        }
        "orchestratorRequired" => {
            fields.active(&["holder", "last_seen"])?;
            ThreadRefusal::OrchestratorRequired {
                thread_id,
                actor,
                holder: fields.holder,
                last_seen: fields.last_seen,
            }
        }
        "orchestratorAlreadyExists" => {
            fields.active(&["holder", "last_seen"])?;
            ThreadRefusal::OrchestratorAlreadyExists {
                thread_id,
                actor,
                holder: required(fields.holder, "reason.holder")?,
                last_seen: required(fields.last_seen, "reason.last_seen")?,
            }
        }
        "implementerAlreadyExists" => {
            fields.active(&["holder", "last_seen"])?;
            ThreadRefusal::ImplementerAlreadyExists {
                thread_id,
                actor,
                holder: required(fields.holder, "reason.holder")?,
                last_seen: required(fields.last_seen, "reason.last_seen")?,
            }
        }
        "orchestratorHandoverRequired" => {
            fields.active(&[])?;
            ThreadRefusal::OrchestratorHandoverRequired { thread_id, actor }
        }
        "handoverTargetNotParticipant" => {
            fields.active(&["target"])?;
            ThreadRefusal::HandoverTargetNotParticipant {
                thread_id,
                actor,
                target: required(fields.target, "reason.target")?,
            }
        }
        "staleOrchestrator" => {
            fields.active(&["holder", "last_seen", "named_holder"])?;
            ThreadRefusal::StaleOrchestrator {
                thread_id,
                actor,
                holder: fields.holder,
                last_seen: fields.last_seen,
                named_holder: required(fields.named_holder, "reason.named_holder")?,
            }
        }
        "staleImplementer" => {
            fields.active(&["holder", "last_seen", "named_holder"])?;
            ThreadRefusal::StaleImplementer {
                thread_id,
                actor,
                holder: fields.holder,
                last_seen: fields.last_seen,
                named_holder: required(fields.named_holder, "reason.named_holder")?,
            }
        }
        "selfReplace" => {
            fields.active(&[])?;
            ThreadRefusal::SelfReplace { thread_id, actor }
        }
        "orchestratorRoleChange" => {
            fields.active(&[])?;
            ThreadRefusal::OrchestratorRoleChange { thread_id, actor }
        }
        _ => return Err(corrupt("reason.thread_kind", "unknown thread refusal")),
    };
    reason
        .validate()
        .map_err(|error| corrupt("reason.thread", error))?;
    Ok(reason)
}
