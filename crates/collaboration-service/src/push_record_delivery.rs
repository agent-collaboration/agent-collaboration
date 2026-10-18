//! Store-first direct-message delivery through Layer 0's PreparedPush path.
use crate::ServiceIdentity;
use automation_storage::StorageError;
use collaboration_protocol::{PushId, PushRecordDraft, PushRecordValidationError};

pub(crate) enum PushDeliveryFailure {
    MailRefused(collaboration_mail_protocol::MailRefusal),
    StoreUnavailable,
    DeliveryUnavailable,
    InvalidRecord(PushRecordValidationError),
    StoreFailed,
    DeliveryUnknown(PushId),
}

pub(crate) async fn store_first_and_deliver(
    draft: PushRecordDraft,
    target_identity: String,
    identity: &ServiceIdentity,
) -> Result<collaboration_protocol::PushMessageSendResult, PushDeliveryFailure> {
    let Some(store) = identity.automation.as_ref() else {
        return Err(PushDeliveryFailure::StoreUnavailable);
    };
    let target = draft.target.clone();
    let push_id = draft.push_id.clone();
    let item_id: collaboration_protocol::MailItemId = push_id
        .clone()
        .try_into()
        .map_err(|_| PushDeliveryFailure::StoreFailed)?;
    draft
        .clone()
        .into_pending()
        .map_err(PushDeliveryFailure::InvalidRecord)?;
    let request = automation_storage::LocalMessageRequest {
        draft,
        target_identity,
    };
    let Some(service) = identity.subscription_delivery.as_ref() else {
        let known = store
            .lock()
            .await
            .replay_local_message(&request)
            .await
            .map_err(storage_failure)?;
        return match known {
            Some(automation_storage::LocalMessageAdmission::Replayed(result)) => Ok(*result),
            Some(automation_storage::LocalMessageAdmission::Pending(id)) => {
                Err(PushDeliveryFailure::DeliveryUnknown(id))
            }
            None => Err(PushDeliveryFailure::DeliveryUnavailable),
        };
    };
    let admission = store
        .lock()
        .await
        .prepare_local_message(&request)
        .await
        .map_err(storage_failure)?;
    if let automation_storage::LocalMessageAdmission::Replayed(result) = admission {
        return Ok(*result);
    }
    service
        .deliver_direct_message(target, push_id.clone())
        .await
        .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?;
    store
        .lock()
        .await
        .read_local_mail(&item_id)
        .await
        .map_err(|_| PushDeliveryFailure::DeliveryUnknown(push_id.clone()))?
        .and_then(|mail| mail.result)
        .ok_or(PushDeliveryFailure::DeliveryUnknown(push_id))
}

fn storage_failure(error: StorageError) -> PushDeliveryFailure {
    match error {
        StorageError::MailRefused(reason) => PushDeliveryFailure::MailRefused(reason),
        StorageError::InvalidRecord => {
            PushDeliveryFailure::InvalidRecord(PushRecordValidationError::DirectMessageTooLarge)
        }
        _ => PushDeliveryFailure::StoreFailed,
    }
}

#[cfg(test)]
mod tests {
    use crate::collaboration_application::CollaborationRejection;
    use crate::{
        BoardAvailability, MachineIdentity, ServiceIdentity, SessionDeliveryRouter,
        SessionMessageDelivery, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
        SystemSubscriptionClock, TargetPresenceProbe,
    };
    use automation_storage::AutomationStore;
    use collaboration_protocol::{
        EndpointId, EndpointRef, MessageContent, MessageDelivery, PushDeliveryState, RouterLink,
        SessionId, SessionMessageSendParams, SessionRef, UuidIdentity,
    };
    use serde_json::{Value, json};
    use std::{path::Path, sync::Arc};
    use tokio::sync::Mutex;

    const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";

    fn session(service_id: &UuidIdentity) -> SessionRef {
        SessionRef {
            endpoint: EndpointRef {
                service_id: service_id.clone(),
                endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("endpoint id"),
            },
            session_id: SessionId::try_from("target-session".to_owned()).expect("session id"),
        }
    }

    async fn automation_store(directory: &Path) -> Arc<Mutex<AutomationStore>> {
        Arc::new(Mutex::new(
            AutomationStore::open(&directory.join("automation.sqlite"))
                .await
                .expect("automation store"),
        ))
    }

    #[tokio::test]
    async fn committed_push_delivery_error_returns_outcome_unknown_with_its_link() {
        let directory = tempfile::tempdir().expect("isolated service directory");
        let automation = automation_store(directory.path()).await;
        let router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
        let delivery: Arc<dyn SessionMessageDelivery> = router.clone();
        let presence: Arc<dyn TargetPresenceProbe> = router;
        let machine_identity = MachineIdentity::new(
            UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service id"),
            Some("delivery-error-test"),
        )
        .expect("machine identity");
        let subscription_service =
            SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
                board_availability: BoardAvailability::Unavailable,
                push_store: Arc::clone(&automation),
                delivery,
                presence: Arc::clone(&presence),
                machine_identity,
                clock: Arc::new(SystemSubscriptionClock),
            });
        subscription_service
            .start()
            .await
            .expect("delivery owner starts");
        subscription_service.shutdown().await;

        let identity = ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .expect("service identity")
            .with_automation_store(Arc::clone(&automation))
            .with_subscription_delivery_service(subscription_service, presence);
        let target = session(&identity.service_id);
        let sent = crate::CollaborationApplication::new(identity.clone())
            .messages()
            .message_send(SessionMessageSendParams {
                item_id: None,
                target: target.clone(),
                message: MessageContent::HumanUser {
                    text: "stored before delivery"
                        .to_owned()
                        .try_into()
                        .expect("message text"),
                },
                mode: MessageDelivery::Auto,
                generation_guard: None,
            })
            .await;
        let response = match sent {
            Ok(result) => json!({ "result": result }),
            Err(failure) => json!({ "error": failure.published_rejection() }),
        };

        let response_message = response
            .pointer("/error/data/message")
            .and_then(Value::as_str)
            .expect("unknown outcome message");
        let link_start = response_message
            .find(&format!("router://{SERVICE_ID}/push/"))
            .expect("unknown outcome message includes a local push link");
        let link_suffix = response_message
            .get(link_start..)
            .expect("push link begins at a UTF-8 boundary");
        let link_end = link_suffix
            .find(" before retrying")
            .expect("push link has an actionable suffix");
        let link_text = link_suffix
            .get(..link_end)
            .expect("push link ends at a UTF-8 boundary");
        let link = RouterLink::parse(link_text).expect("stored push link parses");
        let record = automation
            .lock()
            .await
            .get_push_record(link.push_id())
            .await
            .expect("read committed direct message")
            .expect("push link names the committed record");
        assert_eq!(record.delivery_state, PushDeliveryState::Pending);
        assert!(record.last_outcome.is_none());
        let expected_link = crate::push_record_resolver::link_for(&record, &identity);
        assert_eq!(
            response.pointer("/error/data/kind"),
            Some(&json!("outcomeUnknown"))
        );
        assert_eq!(
            response.pointer("/error/data/stage"),
            Some(&json!("inspect"))
        );
        assert_eq!(
            response.pointer("/error/data/message"),
            Some(&Value::String(format!(
                "Push was stored; delivery outcome is unknown. Inspect {expected_link} before retrying."
            )))
        );
    }
    #[tokio::test]
    async fn recorded_local_result_replays_without_a_delivery_owner_and_unknown_work_is_not_admitted()
     {
        use collaboration_protocol::{
            DeliveryOutcome, DeliveryReceipt, MailItemId, PushHeaderFacts, PushKind, PushOrigin,
            PushRecordDraft,
        };
        let directory = tempfile::tempdir().expect("owned mail fixture");
        let automation = automation_store(directory.path()).await;
        let identity = ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .expect("identity")
            .with_automation_store(automation.clone());
        let target = session(&identity.service_id);
        let id = MailItemId::generate();
        let draft = PushRecordDraft {
            push_id: id.clone().try_into().expect("push ID"),
            kind: PushKind::DirectMessage,
            origin: PushOrigin::OwnerUnverified,
            origin_router_ref: None,
            target: target.clone(),
            mode: Some(MessageDelivery::Auto),
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("retained answer".into()),
            activity: None,
            created_at: chrono::Utc::now(),
        };
        let push_id = draft.push_id.clone();
        automation
            .lock()
            .await
            .prepare_local_message(&automation_storage::LocalMessageRequest {
                draft,
                target_identity: "frozen target".into(),
            })
            .await
            .expect("admission");
        automation
            .lock()
            .await
            .mark_push_attempted(&push_id)
            .await
            .expect("attempt");
        automation
            .lock()
            .await
            .settle_push_record(
                &push_id,
                DeliveryReceipt {
                    outcome: DeliveryOutcome::PeerMessageWritten,
                    reachability: None,
                    client: None,
                },
                chrono::Utc::now(),
            )
            .await
            .expect("terminal outcome");
        let original = automation
            .lock()
            .await
            .read_local_mail(&id)
            .await
            .expect("mail read")
            .expect("mail row")
            .result
            .expect("saved result");
        let app = crate::CollaborationApplication::new(identity);
        let mut input = SessionMessageSendParams {
            item_id: Some(id.clone()),
            target,
            message: MessageContent::HumanUser {
                text: "retained answer".to_owned().try_into().expect("text"),
            },
            mode: MessageDelivery::Auto,
            generation_guard: None,
        };
        let replay = app.messages().message_send(input.clone()).await;
        assert!(
            replay.is_ok(),
            "recorded answer depended on unavailable native delivery: {replay:?}"
        );
        assert_eq!(
            serde_json::to_value(replay.expect("replay")).expect("JSON"),
            serde_json::to_value(original).expect("JSON")
        );
        input.message = MessageContent::HumanUser {
            text: "different content".to_owned().try_into().expect("text"),
        };
        assert_eq!(
            app.messages()
                .message_send(input.clone())
                .await
                .expect_err("conflict")
                .kind,
            crate::collaboration_application::MessageFailureKind::ItemConflict
        );
        let fresh = MailItemId::generate();
        input.item_id = Some(fresh.clone());
        assert_eq!(
            app.messages()
                .message_send(input)
                .await
                .expect_err("no delivery")
                .kind,
            crate::collaboration_application::MessageFailureKind::Unavailable
        );
        assert!(
            automation
                .lock()
                .await
                .read_local_mail(&fresh)
                .await
                .expect("mail read")
                .is_none()
        );
        drop(app);
        Arc::try_unwrap(automation)
            .unwrap_or_else(|_| panic!("mail store retained"))
            .into_inner()
            .close()
            .await
            .expect("close");
    }
}
