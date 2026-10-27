//! Native push projections use frozen mail facts without opening the sender's project.
use chrono::{DateTime, Utc};
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{MailSender, NotificationSource};
use collaboration_protocol::{
    PushActivityRange, PushActivitySnapshot, PushHeaderFacts, PushId, PushKind, PushOrigin,
    PushRecordDraft, RouterOriginRef, SessionRef,
};
use fleet_sync_delivery::DeliveryError;
use fleet_sync_protocol::DeliveryItem;

/// Poll batches are handed to a waiter and have no native push.
pub fn received_mail_push(
    item: &DeliveryItem<RouterDeliveryTypes>,
    received_at: DateTime<Utc>,
) -> Result<Option<PushRecordDraft>, DeliveryError> {
    received_mail_push_for(item, received_at, MailPushPurpose::Receive)
}

pub fn received_poll_mail_push(
    item: &DeliveryItem<RouterDeliveryTypes>,
    received_at: DateTime<Utc>,
) -> Result<PushRecordDraft, DeliveryError> {
    if !is_poll_mail(item) {
        return Err(invalid_projection());
    }
    received_mail_push_for(item, received_at, MailPushPurpose::PollHandover)?
        .ok_or_else(invalid_projection)
}

pub fn is_poll_mail(item: &DeliveryItem<RouterDeliveryTypes>) -> bool {
    matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Notification(body)
        if matches!(&body.source, NotificationSource::FollowBatch { policy, .. }
            if policy.mode() == message_board::SubscriptionMode::Poll))
}

enum MailPushPurpose {
    Receive,
    PollHandover,
}

fn received_mail_push_for(
    item: &DeliveryItem<RouterDeliveryTypes>,
    received_at: DateTime<Utc>,
    purpose: MailPushPurpose,
) -> Result<Option<PushRecordDraft>, DeliveryError> {
    item.body()
        .validate_sender(item.sender())
        .map_err(|_| invalid_projection())?;
    let target: SessionRef = item
        .recipient()
        .address
        .clone()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let push_id: PushId = item
        .id()
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(|_| invalid_projection())?;
    let draft = match item.body() {
        board_sync_protocol::BoardDeliveryBody::Wake(_) => return Ok(None),
        board_sync_protocol::BoardDeliveryBody::Message(body) => {
            let origin = match item.sender() {
                MailSender::Session(sender) => PushOrigin::Session(
                    sender
                        .clone()
                        .try_into()
                        .map_err(|_| invalid_projection())?,
                ),
                MailSender::HumanUser { .. } => PushOrigin::OwnerUnverified,
                MailSender::Project { .. } => return Err(invalid_projection()),
            };
            PushRecordDraft {
                push_id,
                kind: PushKind::DirectMessage,
                origin,
                origin_router_ref: None,
                target,
                mode: Some(body.delivery.into()),
                guard: body
                    .guard
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| invalid_projection())?,
                reply_to_push_id: body
                    .reply_to
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| invalid_projection())?,
                header_facts: PushHeaderFacts::DirectMessage {
                    sender_display_name: body.sender_display_name.clone(),
                },
                body: Some(body.text.as_str().to_owned()),
                activity: None,
                created_at: received_at,
            }
        }
        board_sync_protocol::BoardDeliveryBody::Notification(body) => {
            let (kind, origin, header_facts, activity) = match &body.source {
                NotificationSource::Intent { intent } => (
                    PushKind::BoardNotification,
                    RouterOriginRef::BoardIntent {
                        project: body.pointer.project.clone(),
                        intent: intent.clone(),
                    },
                    PushHeaderFacts::BoardNotification {
                        project: body.pointer.project.clone(),
                        intent: intent.clone(),
                        cause: body.pointer.subject.clone(),
                        summary: board_notification_summary(body.pointer.subject.kind)
                            .to_owned()
                            .try_into()
                            .map_err(|_| invalid_projection())?,
                    },
                    None,
                ),
                NotificationSource::FollowBatch {
                    roots,
                    policy,
                    draining,
                    ..
                } => {
                    if policy.mode() == message_board::SubscriptionMode::Poll {
                        if roots.len() != 1 {
                            return Err(invalid_projection());
                        }
                        if matches!(purpose, MailPushPurpose::Receive) {
                            return Ok(None);
                        }
                    }
                    let message_count = roots
                        .iter()
                        .try_fold(0_u64, |count, root| count.checked_add(root.post_count()))
                        .ok_or_else(invalid_projection)?;
                    let activity = PushActivitySnapshot {
                        ranges: roots
                            .iter()
                            .map(|root| PushActivityRange {
                                thread_id: root.root().clone(),
                                project_id: body.pointer.project.clone(),
                                from: root.from(),
                                through: root.through(),
                            })
                            .collect(),
                        held: false,
                        draining: *draining,
                    };
                    let root_count =
                        u8::try_from(activity.thread_count()).map_err(|_| invalid_projection())?;
                    (
                        PushKind::SubscriptionActivity,
                        RouterOriginRef::SubscriptionActivity {
                            target: target.clone(),
                            batch_id: push_id
                                .as_str()
                                .to_owned()
                                .try_into()
                                .map_err(|_| invalid_projection())?,
                        },
                        PushHeaderFacts::SubscriptionActivity {
                            root_count,
                            message_count,
                            held_since: None,
                            thread_resolved: *draining,
                        },
                        Some(activity),
                    )
                }
            };
            PushRecordDraft {
                push_id,
                kind,
                origin: PushOrigin::Router(kind),
                origin_router_ref: Some(
                    origin
                        .canonical_string()
                        .map_err(|_| invalid_projection())?,
                ),
                target,
                mode: None,
                guard: None,
                reply_to_push_id: None,
                header_facts,
                body: None,
                activity,
                created_at: received_at,
            }
        }
    };
    // Validate the same native shape used by storage, including activity bounds.
    draft
        .clone()
        .into_pending()
        .map_err(|_| invalid_projection())?;
    Ok(Some(draft))
}

pub const fn board_notification_summary(cause: message_board::RecordKind) -> &'static str {
    match cause {
        message_board::RecordKind::Post => "New thread post",
        message_board::RecordKind::RoleAssignment => "Role assignment changed",
        _ => "New board activity",
    }
}

fn invalid_projection() -> DeliveryError {
    DeliveryError::Corrupt {
        field: "received mail push",
    }
}

#[cfg(test)]
#[path = "received_mail_push_tests.rs"]
mod tests;
