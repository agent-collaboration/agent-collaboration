//! Adapt SDK delivery calls onto the already prepared machine session.
use super::{
    AtomicStore, CallEnvelope, CallFailure, CallTarget, DeliveryItemRef, MailCallRouter, MailHost,
    MailSender, NotSentReason, RouterDeliveryTypes, StoreError, TursoScope, Value, Weak,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardDeliveryTypes, MailSubmitAnswer,
};
use fleet_sync_delivery::{
    DeliveryTransactionFailure, FetchCall, FetchInput, Outbox, ReceiptInput, ReceivedCall,
    ReportCall, SubmitCall, classify_delivery_store_error, delivery_rollback,
    submit_settlement_outcome,
};
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryEnvelope, DeliveryRequest, OperationAnswer, OperationRequest,
};
use serde::{Serialize, de::DeserializeOwned};

fn unavailable() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

fn decode_call<T: DeserializeOwned>(value: Value) -> Result<T, CallFailure> {
    serde_json::from_value(value).map_err(|_| unavailable())
}

fn encode_answer(answer: impl Serialize) -> Result<Value, CallFailure> {
    serde_json::to_value(answer).map_err(|_| CallFailure::AnswerLost)
}

async fn recorded_submission_epoch(
    scope: &mut TursoScope<'_, MailHost>,
    envelope: &DeliveryEnvelope<RouterDeliveryTypes>,
) -> Result<Option<fleet_sync_protocol::Epoch>, StoreError> {
    let MailSender::Project { project } = envelope.sender() else {
        return Ok(None);
    };
    let item = envelope
        .id()
        .as_uuid()
        .to_string()
        .try_into()
        .map_err(StoreError::backend)?;
    let recorded = interaction_storage::read_mail_submission_epoch_in_scope(scope, &item)
        .await
        .map_err(StoreError::backend)?
        .ok_or_else(|| {
            tracing::error!(item = %envelope.id().as_uuid(), project = project.as_str(),
                "project mail has no recorded submission epoch; retaining custody for retry");
            StoreError::backend("project mail submission epoch is missing")
        })?;
    if recorded.project != *project {
        return Err(StoreError::backend(
            "project mail submission epoch belongs to another project",
        ));
    }
    fleet_sync_protocol::Epoch::new(recorded.epoch.get())
        .map(Some)
        .map_err(StoreError::backend)
}

impl MailCallRouter {
    pub(super) async fn dispatch_remote(
        &self,
        envelope: CallEnvelope,
    ) -> Result<Value, CallFailure> {
        if envelope.kind == super::terminal_retirement::RetireMailCall::KIND {
            return self.dispatch_retirement(envelope).await;
        }
        let session = self.session.as_ref().ok_or_else(unavailable)?;
        let CallEnvelope {
            kind,
            target,
            request,
            call,
        } = envelope;
        match kind {
            SubmitCall::<RouterDeliveryTypes>::KIND => {
                let target: SubmitCall<RouterDeliveryTypes> = decode_call(target)?;
                if target.relay != session.relay {
                    return Err(unavailable());
                }
                let reference: DeliveryItemRef = decode_call(request)?;
                let lookup = reference.clone();
                let config = self.config;
                let store = self
                    .store
                    .get()
                    .and_then(Weak::upgrade)
                    .ok_or_else(unavailable)?;
                let resolved = store
                    .transaction(move |scope| {
                        Box::pin(async move {
                            let resolved =
                                Outbox::<_, RouterDeliveryTypes>::new(scope, config.limits)
                                    .resolve_submit(&lookup)
                                    .await
                                    .map_err(delivery_rollback)?;
                            let origin_epoch =
                                recorded_submission_epoch(scope, &resolved.item).await?;
                            Ok((resolved, origin_epoch))
                        })
                    })
                    .await;
                match resolved {
                    Ok((resolved, origin_epoch)) => {
                        if resolved.relay != target.relay
                            || resolved.item.recipient().peer != target.mailbox
                        {
                            return Err(unavailable());
                        }
                        // The frozen recipient pair came from enrollment at producer admission.
                        // Only the local observation marker differs from the shared wire marker.
                        let recipient_service = resolved
                            .item
                            .recipient()
                            .address
                            .endpoint
                            .service_id
                            .clone();
                        let project = match resolved.item.sender() {
                            MailSender::Project { project } => Some(project.clone()),
                            _ => None,
                        };
                        let item: DeliveryEnvelope<BoardDeliveryTypes> =
                            serde_json::from_value(encode_answer(resolved.item)?)
                                .map_err(|_| CallFailure::AnswerLost)?;
                        let request = BoardConsumerRequest::mail_submit(
                            item,
                            origin_epoch,
                            &recipient_service,
                            target.mailbox,
                        )
                        .map_err(|_| unavailable())?;
                        match session
                            .requests
                            .request(OperationRequest::Consumer(request))
                            .await?
                        {
                            OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                                MailSubmitAnswer::Submitted(answer),
                            )) => encode_answer(fleet_sync_delivery::DispatchOutcome::Answered(
                                answer,
                            )),
                            OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                                MailSubmitAnswer::NotWriter,
                            )) => {
                                if let Some(project) = project {
                                    let confirmed = match self.project_replication.get() {
                                        Some(owner) => owner.confirmed_writer_epoch(&project).await,
                                        None => None,
                                    };
                                    if origin_epoch.is_some() && confirmed == origin_epoch {
                                        tracing::error!(project = project.as_str(), submitted_epoch = origin_epoch.map(|epoch|epoch.value()),
                                            own_peer = %self.config.owner.as_uuid(), confirmed_epoch = confirmed.map(|epoch|epoch.value()),
                                            "central rejected project mail while this holder is confirmed");
                                    } else {
                                        tracing::warn!(project = project.as_str(), submitted_epoch = origin_epoch.map(|epoch|epoch.value()),
                                            own_peer = %self.config.owner.as_uuid(), confirmed_epoch = confirmed.map(|epoch|epoch.value()),
                                            "central rejected project mail authority; custody retained");
                                    }
                                }
                                Err(CallFailure::AnswerLost)
                            }
                            OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                                MailSubmitAnswer::MailboxUnavailable { .. },
                            )) => Err(CallFailure::AnswerLost),
                            _ => Err(CallFailure::AnswerLost),
                        }
                    }
                    Err(error) => {
                        match classify_delivery_store_error(error) {
                            DeliveryTransactionFailure::OutcomeUnknown(_) => {
                                return Err(CallFailure::AnswerLost);
                            }
                            DeliveryTransactionFailure::Refused(_)
                            | DeliveryTransactionFailure::Corrupt { .. } => {}
                        }
                        match session
                            .requests
                            .request(OperationRequest::Delivery(DeliveryRequest::Settle {
                                call,
                                mailbox: target.mailbox,
                                item: reference.item,
                                digest: reference.digest,
                            }))
                            .await?
                        {
                            OperationAnswer::Delivery(DeliveryAnswer::Settle(answer)) => {
                                encode_answer(submit_settlement_outcome(answer))
                            }
                            _ => Err(CallFailure::AnswerLost),
                        }
                    }
                }
            }
            FetchCall::<RouterDeliveryTypes>::KIND => {
                let target: FetchCall<RouterDeliveryTypes> = decode_call(target)?;
                if target.relay != session.relay || target.mailbox != self.config.owner {
                    return Err(unavailable());
                }
                let request: FetchInput = decode_call(request)?;
                match session
                    .requests
                    .request(OperationRequest::Delivery(DeliveryRequest::Fetch {
                        call,
                        stream: request.stream,
                        after: request.after,
                        max_items: request.max_items,
                        max_bytes: request.max_bytes,
                        inbox_room_bytes: request.inbox_room_bytes,
                    }))
                    .await?
                {
                    OperationAnswer::Delivery(DeliveryAnswer::Fetch(answer)) => {
                        encode_answer(answer)
                    }
                    _ => Err(CallFailure::AnswerLost),
                }
            }
            ReceivedCall::KIND => {
                let target: ReceivedCall = decode_call(target)?;
                if target.relay != session.relay || target.mailbox != self.config.owner {
                    return Err(unavailable());
                }
                let request: ReceiptInput = decode_call(request)?;
                match session
                    .requests
                    .request(OperationRequest::Delivery(DeliveryRequest::Received {
                        call,
                        stream: request.stream,
                        through: request.through,
                    }))
                    .await?
                {
                    OperationAnswer::Delivery(DeliveryAnswer::Received(answer)) => {
                        encode_answer(answer)
                    }
                    _ => Err(CallFailure::AnswerLost),
                }
            }
            ReportCall::<RouterDeliveryTypes>::KIND => {
                let target: ReportCall<RouterDeliveryTypes> = decode_call(target)?;
                if target.relay != session.relay || target.mailbox != self.config.owner {
                    return Err(unavailable());
                }
                let report = decode_call(request)?;
                match session
                    .requests
                    .request(OperationRequest::Delivery(DeliveryRequest::Report {
                        call,
                        report,
                    }))
                    .await?
                {
                    OperationAnswer::Delivery(DeliveryAnswer::Report(answer)) => {
                        encode_answer(answer)
                    }
                    _ => Err(CallFailure::AnswerLost),
                }
            }
            _ => Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            }),
        }
    }
}
