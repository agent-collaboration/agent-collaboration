//! The existing show entry reads sender Outbox status without inventing a push.
use super::{
    MessageFailure, MessageFailureKind, MessageFailureStage, MessageOperations, PushHeaderFacts,
    PushKind, PushOrigin, PushRecordHistoryParams, PushRecordShowParams, PushRecordShowResult,
    StorageError, caller_is_local, expand_activity_ranges, link_for,
};
use collaboration_protocol::{
    MessageShowResult, RemoteDeliveryRefusal, RemoteDeliveryStatus, RemoteMessageStatus,
    RemoteProcessingFailure, RemoteSizeField,
};
use fleet_sync_delivery::OutboxStatusState;
use fleet_sync_protocol::{
    DeliveryFailureKind, DeliveryRefusal, DeliverySizeField, DeliveryStatus, ItemId,
};
use interaction_storage::DirectMessageHistoryQuery;

impl MessageOperations {
    /// Lists direct-message history, including outgoing SDK custody, for this caller.
    pub async fn message_history(
        &self,
        request: PushRecordHistoryParams,
    ) -> Result<collaboration_protocol::MessageHistoryResult, MessageFailure> {
        if !caller_is_local(&request.caller, &self.identity) {
            return Err(MessageFailure::discovery(
                MessageFailureKind::WrongService,
                "Caller must belong to this Router",
            ));
        }
        let store = self.push_store(MessageFailureStage::Discovery)?;
        let records = store
            .list_direct_message_history(&DirectMessageHistoryQuery {
                caller: request.caller.clone(),
                with: request.with.clone(),
                limit: request.limit,
            })
            .await
            .map_err(|error| match error {
                StorageError::InvalidRecord => MessageFailure::discovery(
                    MessageFailureKind::InvalidField,
                    "History limit must be between 1 and 100",
                ),
                _ => MessageFailure::discovery(
                    MessageFailureKind::Unavailable,
                    "Push history could not be read",
                ),
            })?;
        let mut records = self
            .notice_list(records)?
            .records
            .into_iter()
            .map(collaboration_protocol::MessageHistoryEntry::Local)
            .collect::<Vec<_>>();
        let outgoing = store
            .direct_mail_read_contexts(&request.caller, &request.with, request.limit)
            .await
            .map_err(|_| {
                MessageFailure::discovery(
                    MessageFailureKind::Unavailable,
                    "Outgoing history could not be read",
                )
            })?;
        for context in outgoing {
            let shown = self
                .message_show(PushRecordShowParams {
                    caller: request.caller.clone(),
                    reference: context.item.as_str().to_owned(),
                })
                .await?;
            if let collaboration_protocol::MessageShowResult::Remote { mail } = shown {
                records.push(collaboration_protocol::MessageHistoryEntry::Remote { mail: *mail });
            }
        }
        let key = |record: &collaboration_protocol::MessageHistoryEntry| match record {
            collaboration_protocol::MessageHistoryEntry::Local(record) => {
                (record.created_at, record.push_id.as_str().to_owned())
            }
            collaboration_protocol::MessageHistoryEntry::Remote { mail } => {
                (mail.recorded_at, mail.item.as_str().to_owned())
            }
        };
        records.sort_by_key(|record| std::cmp::Reverse(key(record)));
        records.truncate(usize::try_from(request.limit).map_err(|_| {
            MessageFailure::discovery(MessageFailureKind::InvalidField, "Invalid history limit")
        })?);
        Ok(collaboration_protocol::MessageHistoryResult { records })
    }

    pub async fn message_show(
        &self,
        request: impl Into<collaboration_protocol::MessageShowRequest>,
    ) -> Result<MessageShowResult, MessageFailure> {
        use collaboration_protocol::MessageReadCaller;
        let request = request.into();
        let stage = MessageFailureStage::Inspect;
        if let MessageReadCaller::Session(caller) = &request.caller
            && !caller_is_local(caller, &self.identity)
        {
            return Err(MessageFailure::new(
                MessageFailureKind::WrongService,
                stage,
                "Caller belongs to another Router",
            ));
        }
        let push = self.resolve(&request.reference, stage)?;
        let item: collaboration_protocol::MailItemId = push
            .clone()
            .try_into()
            .map_err(|_| MessageFailure::not_found(stage))?;
        let store = self.push_store(stage)?;
        let context = store.mail_read_context(&item).await.map_err(|_| {
            MessageFailure::new(
                MessageFailureKind::Unavailable,
                stage,
                "Sender read context could not be read",
            )
        })?;
        if let Some(context) = context {
            self.authorize_mail_read(&request.caller, &context.context)
                .await?;
            let mail = self.mail_delivery.as_ref().ok_or_else(|| {
                MessageFailure::new(
                    MessageFailureKind::Unavailable,
                    stage,
                    "Sender custody is unavailable",
                )
            })?;
            let sdk_item = ItemId::new(
                uuid::Uuid::parse_str(item.as_str())
                    .map_err(|_| MessageFailure::not_found(stage))?,
            )
            .map_err(|_| MessageFailure::not_found(stage))?;
            let status = mail.outbox_status(sdk_item).await.map_err(|_| {
                MessageFailure::new(
                    MessageFailureKind::Unavailable,
                    stage,
                    "Sender custody could not be read",
                )
            })?;
            let OutboxStatusState::Known { status, .. } = status else {
                return Err(MessageFailure::not_found(stage));
            };
            let status = public_status(status).map_err(|_| {
                MessageFailure::new(
                    MessageFailureKind::Unavailable,
                    stage,
                    "Sender status is invalid",
                )
            })?;
            return Ok(MessageShowResult::Remote {
                mail: Box::new(RemoteMessageStatus {
                    item,
                    context: context.context,
                    recorded_at: context.recorded_at,
                    status,
                }),
            });
        }
        match request.caller {
            MessageReadCaller::Session(caller) => self
                .push_show(PushRecordShowParams {
                    caller,
                    reference: request.reference,
                })
                .await
                .map(|shown| MessageShowResult::Local(Box::new(shown))),
            MessageReadCaller::Human(_) => {
                let record = store
                    .get_push_record(&push)
                    .await
                    .map_err(|_| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Push storage could not be read",
                        )
                    })?
                    .ok_or_else(|| MessageFailure::not_found(stage))?;
                if !(record.kind == PushKind::DirectMessage
                    && record.origin == PushOrigin::OwnerUnverified
                    || matches!(
                        record.header_facts,
                        PushHeaderFacts::BoardNotification { .. }
                    ))
                {
                    return Err(MessageFailure::new(
                        MessageFailureKind::NotPermitted,
                        stage,
                        "The owner caller cannot read this push kind",
                    ));
                }
                let activity_ranges = expand_activity_ranges(&record, self.registry.as_ref())
                    .await
                    .map_err(|_| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Stored activity could not be read",
                        )
                    })?;
                Ok(MessageShowResult::Local(Box::new(PushRecordShowResult {
                    link: link_for(&record, &self.identity),
                    record,
                    activity_ranges,
                })))
            }
        }
    }

    async fn authorize_mail_read(
        &self,
        caller: &collaboration_protocol::MessageReadCaller,
        context: &collaboration_protocol::MailReadContext,
    ) -> Result<(), MessageFailure> {
        use collaboration_protocol::{MailReadContext, MessageReadCaller};
        let stage = MessageFailureStage::Inspect;
        let allowed = match context {
            MailReadContext::Direct { origin, recipient } => match caller {
                MessageReadCaller::Session(caller) => {
                    caller == recipient
                        || matches!(origin,PushOrigin::Session(sender) if sender == caller)
                }
                MessageReadCaller::Human(_) => *origin == PushOrigin::OwnerUnverified,
            },
            MailReadContext::Project { project, .. } => match caller {
                MessageReadCaller::Human(_) => true,
                MessageReadCaller::Session(caller) => {
                    let actor = message_board::Identity::Session {
                        session: caller.clone().try_into().map_err(|_| {
                            MessageFailure::new(
                                MessageFailureKind::InvalidField,
                                stage,
                                "Invalid caller identity",
                            )
                        })?,
                    };
                    let registry = self.registry.as_ref().ok_or_else(|| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Project authority is unavailable",
                        )
                    })?;
                    let store = registry
                        .read_project(project)
                        .await
                        .map_err(|_| {
                            MessageFailure::new(
                                MessageFailureKind::Unavailable,
                                stage,
                                "Project authority could not be read",
                            )
                        })?
                        .ok_or_else(|| MessageFailure::not_found(stage))?;
                    let mut read = store.snapshot().await.map_err(|_| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Project authority could not be read",
                        )
                    })?;
                    let subject = message_board::RoleSubject::Project(project.clone());
                    let keys = read
                        .active_role_selection_keys(&subject, message_board::AuthorityRole::Lead)
                        .await
                        .map_err(|_| {
                            MessageFailure::new(
                                MessageFailureKind::Unavailable,
                                stage,
                                "Project lead could not be read",
                            )
                        })?;
                    let mut allowed = false;
                    for key in keys {
                        let Some(message_board::Record::RoleAssignment(role)) =
                            read.record(&key).await.map_err(|_| {
                                MessageFailure::new(
                                    MessageFailureKind::Unavailable,
                                    stage,
                                    "Project lead is invalid",
                                )
                            })?
                        else {
                            return Err(MessageFailure::new(
                                MessageFailureKind::Unavailable,
                                stage,
                                "Project lead is invalid",
                            ));
                        };
                        if role.subject() != subject
                            || role.role() != message_board::AuthorityRole::Lead
                            || role.ended.is_some()
                        {
                            return Err(MessageFailure::new(
                                MessageFailureKind::Unavailable,
                                stage,
                                "Project lead is invalid",
                            ));
                        }
                        allowed |= role.holder.declared_identity().as_ref() == Some(&actor);
                    }
                    read.finish().await.map_err(|_| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Project read could not finish",
                        )
                    })?;
                    allowed
                }
            },
        };
        if allowed {
            Ok(())
        } else {
            Err(MessageFailure::new(
                MessageFailureKind::NotPermitted,
                stage,
                "Caller cannot read this outgoing mail item",
            ))
        }
    }
}

fn public_status(
    status: DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>,
) -> Result<RemoteDeliveryStatus, ()> {
    Ok(match status {
        DeliveryStatus::Queued => RemoteDeliveryStatus::Queued,
        DeliveryStatus::Sent { seq } => RemoteDeliveryStatus::Sent { seq: seq.value() },
        DeliveryStatus::AtRecipient => RemoteDeliveryStatus::AtMachine,
        DeliveryStatus::Held => RemoteDeliveryStatus::Held,
        DeliveryStatus::Delivered { acceptance } => RemoteDeliveryStatus::Delivered { acceptance },
        DeliveryStatus::Uncertain => RemoteDeliveryStatus::Uncertain,
        DeliveryStatus::Expired => RemoteDeliveryStatus::Expired,
        DeliveryStatus::Cancelled => RemoteDeliveryStatus::Cancelled,
        DeliveryStatus::Refused { reason } => RemoteDeliveryStatus::Refused {
            reason: match reason {
                DeliveryRefusal::RecipientRefused { code } => {
                    RemoteDeliveryRefusal::RecipientRefused {
                        code: code.as_str().to_owned(),
                    }
                }
                DeliveryRefusal::ProcessingFailed { kind } => {
                    RemoteDeliveryRefusal::ProcessingFailed {
                        failure: match kind {
                            DeliveryFailureKind::CustodyMissing => {
                                RemoteProcessingFailure::CustodyMissing
                            }
                            DeliveryFailureKind::CustodyUnresolvable => {
                                RemoteProcessingFailure::CustodyUnresolvable
                            }
                            DeliveryFailureKind::CustodyCorrupt => {
                                RemoteProcessingFailure::CustodyCorrupt
                            }
                            DeliveryFailureKind::DigestMismatch => {
                                RemoteProcessingFailure::DigestMismatch
                            }
                            DeliveryFailureKind::VersionMismatch => {
                                RemoteProcessingFailure::VersionMismatch
                            }
                            DeliveryFailureKind::Nondeterministic => {
                                RemoteProcessingFailure::Nondeterministic
                            }
                            DeliveryFailureKind::InvalidPayload => {
                                RemoteProcessingFailure::InvalidPayload
                            }
                            DeliveryFailureKind::ExecutionFailed => {
                                RemoteProcessingFailure::ExecutionFailed
                            }
                        },
                    }
                }
                DeliveryRefusal::TooLarge { field } => RemoteDeliveryRefusal::TooLarge {
                    field: match field {
                        DeliverySizeField::Sender => RemoteSizeField::Sender,
                        DeliverySizeField::Address => RemoteSizeField::Address,
                        DeliverySizeField::Acceptance => RemoteSizeField::Acceptance,
                        DeliverySizeField::StatusRoute => RemoteSizeField::StatusRoute,
                    },
                },
                DeliveryRefusal::MailboxFull => RemoteDeliveryRefusal::MailboxFull,
                DeliveryRefusal::ItemConflict { existing } => RemoteDeliveryRefusal::ItemConflict {
                    existing: existing.as_str().to_owned().try_into().map_err(|_| ())?,
                },
                DeliveryRefusal::CancelAlreadyRequested { existing } => {
                    RemoteDeliveryRefusal::CancelAlreadyRequested {
                        cancel: existing
                            .item
                            .as_uuid()
                            .to_string()
                            .try_into()
                            .map_err(|_| ())?,
                        digest: existing
                            .digest
                            .as_str()
                            .to_owned()
                            .try_into()
                            .map_err(|_| ())?,
                    }
                }
                DeliveryRefusal::RecipientUnknown => RemoteDeliveryRefusal::RecipientUnknown,
                DeliveryRefusal::RecipientRevoked => RemoteDeliveryRefusal::RecipientRevoked,
                DeliveryRefusal::DroppedByPolicy => RemoteDeliveryRefusal::DroppedByPolicy,
                DeliveryRefusal::CancelSenderMismatch => {
                    RemoteDeliveryRefusal::CancelSenderMismatch
                }
            },
        },
    })
}
