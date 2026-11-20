//! The existing show entry reads sender Outbox status without inventing a push.
use super::{
    MessageFailure, MessageFailureKind, MessageFailureStage, MessageOperations, PushHeaderFacts,
    PushKind, PushOrigin, PushRecordHistoryParams, PushRecordShowParams, PushRecordShowResult,
    StorageError, caller_is_local, link_for,
};
use collaboration_protocol::MessageShowResult;
use fleet_sync_protocol::ItemId;
use interaction_storage::DirectMessageHistoryQuery;

impl MessageOperations {
    /// Lists direct-message history, for this caller.
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
        let records = self
            .notice_list(records)?
            .records
            .into_iter()
            .map(collaboration_protocol::MessageHistoryEntry::Local)
            .collect::<Vec<_>>();
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
        let sdk_item = ItemId::new(
            uuid::Uuid::parse_str(push.as_str()).map_err(|_| MessageFailure::not_found(stage))?,
        )
        .map_err(|_| MessageFailure::not_found(stage))?;
        match request.caller {
            MessageReadCaller::Session(caller) => self
                .push_show(PushRecordShowParams {
                    caller,
                    reference: request.reference,
                })
                .await
                .map(|shown| MessageShowResult::Local(Box::new(shown)))
                .map_err(|failure| {
                    if failure.kind == MessageFailureKind::NotFound {
                        self.missing_status(sdk_item, stage)
                    } else {
                        failure
                    }
                }),
            MessageReadCaller::Human(_) => {
                let record = self
                    .push_store(stage)?
                    .get_push_record(&push)
                    .await
                    .map_err(|_| {
                        MessageFailure::new(
                            MessageFailureKind::Unavailable,
                            stage,
                            "Push storage could not be read",
                        )
                    })?
                    .ok_or_else(|| self.missing_status(sdk_item, stage))?;
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
                Ok(MessageShowResult::Local(Box::new(PushRecordShowResult {
                    link: link_for(&record, &self.identity),
                    record,
                    activity_ranges: Vec::new(),
                })))
            }
        }
    }

    fn missing_status(&self, item: ItemId, stage: MessageFailureStage) -> MessageFailure {
        let Some(mail) = &self.mail_delivery else {
            return MessageFailure::not_found(stage);
        };
        let message = if mail.custody_record_is_past_retention(item) {
            "removed after 60 days"
        } else {
            "unknown"
        };
        MessageFailure::new(MessageFailureKind::NotFound, stage, message)
    }
}
