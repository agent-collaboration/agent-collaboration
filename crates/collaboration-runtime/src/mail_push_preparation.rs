//! Frozen mail chooses native policy; only held metadata comes from the local push.
use crate::received_mail_push;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::UnsentCause;
use collaboration_contracts::layer_zero::DeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::{
    AttemptId, DeliveryCorrelationId, DeliveryNextAction, DeliveryOutcome, DeliveryReceipt,
    DeliveryRejection, DeliveryRejectionReason, MachineId, MessageDelivery, MessageText, PushKind,
    PushLineInput, PushRecord, RouterLink, render_push_line,
};
use fleet_sync_delivery::DeliveryError;
use fleet_sync_protocol::DeliveryItem;
use runtime_primitives::CallId;

/// The library caller owns admission, attempt journaling and outcome persistence.
pub enum MailPushPreparation {
    Attempt {
        kind: PushKind,
        request: Box<DeliveryRequest>,
    },
    Hold {
        receipt: DeliveryReceipt,
    },
    Refuse {
        delivery: NativeDeliveryResult,
    },
}

/// Released Auto policy is shared by restored pushes and the frozen mail path.
pub fn direct_message_load_policy(
    mode: Option<MessageDelivery>,
    guard: Option<&collaboration_protocol::CodexGeneration>,
    state: collaboration_protocol::PushDeliveryState,
) -> LoadPolicy {
    if mode == Some(MessageDelivery::Auto)
        && state == collaboration_protocol::PushDeliveryState::Pending
        && guard.is_none()
    {
        LoadPolicy::MayLoad
    } else {
        LoadPolicy::LoadedOnly
    }
}

#[cfg(test)]
fn prepare_mail_push(
    item: &DeliveryItem<RouterDeliveryTypes>,
    working_copy: &PushRecord,
    machine: &MachineIdentity,
    presence: &TargetPresence,
    call: &CallId,
) -> Result<Option<MailPushPreparation>, DeliveryError> {
    prepare_mail_push_with_load_policy(
        item,
        working_copy,
        machine,
        presence,
        call,
        LoadPolicy::LoadedOnly,
    )
}

pub fn prepare_mail_push_with_load_policy(
    item: &DeliveryItem<RouterDeliveryTypes>,
    working_copy: &PushRecord,
    machine: &MachineIdentity,
    presence: &TargetPresence,
    call: &CallId,
    direct_load_policy: LoadPolicy,
) -> Result<Option<MailPushPreparation>, DeliveryError> {
    if !matches!(item.body(), BoardDeliveryBody::Message(_)) {
        return Err(invalid_preparation());
    }
    let Some(draft) = received_mail_push(item, working_copy.created_at)? else {
        return Ok(None);
    };
    if draft.push_id != working_copy.push_id
        || draft.kind != working_copy.kind
        || draft.target != working_copy.target
        || &draft.target.endpoint.service_id != machine.service_id()
    {
        return Err(invalid_preparation());
    }
    let policy = direct_load_policy;
    // Unreachable is ambiguous: the native router must classify live-elsewhere
    // and permanent route failures, just as the existing direct-message owner does.
    if matches!(presence, TargetPresence::Wakeable) {
        if draft.guard.is_some() || draft.mode == Some(MessageDelivery::Steer) {
            let (reason, detail) = if draft.guard.is_some() {
                (
                    DeliveryRejectionReason::StaleGeneration,
                    "guarded direct message requires a running target generation",
                )
            } else {
                (
                    DeliveryRejectionReason::SteerUnsupported,
                    "steer direct message requires a running target",
                )
            };
            return Ok(Some(MailPushPreparation::Refuse {
                delivery: NativeDeliveryResult::rejected(
                    DeliveryRejection {
                        native_failure: None,
                        reason,
                        next_action: DeliveryNextAction::InspectTarget,
                        client_code: None,
                        detail: Some(detail.to_owned()),
                        claims: None,
                    },
                    UnsentCause::for_rejection(reason),
                ),
            }));
        }
        if policy != LoadPolicy::MayLoad {
            return Ok(Some(MailPushPreparation::Hold {
                receipt: not_submitted(true, "target is not running; direct message held"),
            }));
        }
    }
    let load_policy = policy;
    let line = render_push_line(&PushLineInput {
        link: RouterLink::new(
            MachineId::from(machine.service_id().clone()),
            draft.push_id.clone(),
        ),
        machine_label: machine.machine_label().clone(),
        origin: draft.origin,
        header_facts: draft.header_facts,
        body: draft.body,
    })
    .map_err(|_| invalid_preparation())?;
    let request = DeliveryRequest {
        payload: PreparedPush {
            push_id: draft.push_id.clone(),
            line: MessageText::try_from(line).map_err(|_| invalid_preparation())?,
            load_policy,
        },
        target: draft.target,
        mode: draft.mode.unwrap_or(MessageDelivery::Auto),
        precondition: draft
            .guard
            .map_or(DeliveryPrecondition::Unpinned, |expected| {
                DeliveryPrecondition::EndpointGeneration { expected }
            }),
        correlation: DeliveryCorrelationId::try_from(draft.push_id.as_str().to_owned())
            .map_err(|_| invalid_preparation())?,
        // DeliverLocally has one external call per operation. Recovery must retain
        // its attempt identity rather than mint a second native attempt.
        attempt: AttemptId::try_from(call.operation.as_uuid().to_string())
            .map_err(|_| invalid_preparation())?,
    };
    Ok(Some(MailPushPreparation::Attempt {
        kind: draft.kind,
        request: Box::new(request),
    }))
}

fn not_submitted(retryable: bool, reason: &str) -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable,
            reason: reason.to_owned(),
        },
        reachability: None,
        client: None,
    }
}

fn invalid_preparation() -> DeliveryError {
    DeliveryError::Corrupt {
        field: "native mail push preparation",
    }
}

#[cfg(test)]
#[path = "mail_push_preparation_tests.rs"]
mod tests;
