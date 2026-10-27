//! Native SDK custody precedes direct-message delivery through the reader's actor.
use collaboration_protocol::{PushId, PushRecordDraft, PushRecordValidationError};

#[derive(Debug)]
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
    interaction_store: Option<&std::sync::Arc<interaction_storage::InteractionStore>>,
    mail_delivery: Option<&std::sync::Arc<crate::MailDeliveryRuntime>>,
) -> Result<collaboration_protocol::PushMessageSendResult, PushDeliveryFailure> {
    if interaction_store.is_none() {
        return Err(PushDeliveryFailure::StoreUnavailable);
    }
    let Some(runtime) = mail_delivery else {
        return Err(PushDeliveryFailure::DeliveryUnavailable);
    };
    runtime.send_local(draft, target_identity).await
}

#[cfg(test)]
mod tests {
    use crate::BoardAvailability;
    use crate::SubscriptionDeliveryService;
    use crate::SubscriptionDeliveryServiceProps;
    use crate::SystemSubscriptionClock;
    use crate::collaboration_application::CollaborationRejection;
    use crate::ApplicationFixture;

    use collaboration_contracts::MachineIdentity;
    use collaboration_runtime::SessionDeliveryRouter;
    use collaboration_contracts::SessionMessageDelivery;



    use collaboration_contracts::TargetPresenceProbe;
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

    async fn identity_without_native_work(
        owner: Arc<interaction_storage::InteractionStore>,
        peer: fleet_sync_protocol::PeerId,
        automation: Arc<Mutex<AutomationStore>>,
    ) -> ApplicationFixture {
        let router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
        let delivery: Arc<dyn SessionMessageDelivery> = router.clone();
        let presence: Arc<dyn TargetPresenceProbe> = router;
        let machine = MachineIdentity::new(
            UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
            Some("delivery-error-test"),
        )
        .unwrap();
        let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: owner.clone(),
            delivery,
            presence: presence.clone(),
            machine_identity: machine.clone(),
            clock: Arc::new(SystemSubscriptionClock),
        });
        actors.start().await.unwrap();
        actors.shutdown().await;
        let mail = crate::MailDeliveryRuntime::new(
            peer,
            crate::MailPushDeliveryProps {
                interactions: owner.clone(),
                actors: actors.clone(),
                presence: presence.clone(),
                machine,
                clock: Arc::new(SystemSubscriptionClock),
            },
            None,
        )
        .unwrap()
        .without_registered_task_for_test();
        ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_automation_store(automation)
            .with_subscription_delivery_service(actors, presence)
            .with_interaction_store(owner)
            .with_installation_peer(peer)
            .unwrap()
            .with_mail_delivery_runtime(Arc::new(mail))
            .unwrap()
    }

    #[tokio::test]
    async fn committed_push_delivery_error_returns_outcome_unknown_with_its_link() {
        let directory = tempfile::tempdir().expect("isolated service directory");
        let automation = automation_store(directory.path()).await;
        let owner = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await
            .unwrap(),
        );
        let peer = fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).unwrap();
        // Native work is unavailable. The real SDK's missing registration is the
        // post-commit error; no synthetic MessageFailure or fake receipt is supplied.
        let identity = identity_without_native_work(owner.clone(), peer, automation.clone()).await;
        let target = session(&identity.service_id);
        let sent = (identity.clone())
            .build()
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
        let record = owner
            .get_push_record(link.push_id())
            .await
            .expect("read committed direct message")
            .expect("push link names the committed record");
        assert_eq!(record.delivery_state, PushDeliveryState::Pending);
        assert!(record.last_outcome.is_none());
        let expected_link =
            crate::push_record_resolver::link_for(&record, &identity.router_identity());
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
        drop(identity);
        Arc::try_unwrap(owner)
            .unwrap_or_else(|_| panic!("native owner retained"))
            .close()
            .await
            .unwrap();
        Arc::try_unwrap(automation)
            .unwrap_or_else(|_| panic!("automation owner retained"))
            .into_inner()
            .close()
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn recorded_local_result_replays_without_a_delivery_owner_and_unknown_work_is_not_admitted()
     {
        use crate::mail_delivery_projection_tests::fixture::Fixture;
        use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, MailItemId};
        let fixture = Fixture::new().await;
        let directory = tempfile::tempdir().expect("owned actor fixture");
        let automation = automation_store(directory.path()).await;
        let mut draft = fixture.draft();
        draft.body = Some("retained answer".to_owned());
        let target = draft.target.clone();
        let id: MailItemId = draft.push_id.clone().try_into().unwrap();
        fixture
            .admit_local(draft)
            .await
            .expect("real SDK admission");
        fixture.push_answer(
            fleet_sync_delivery::LocalOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::PeerMessageWritten,
            },
            DeliveryReceipt {
                outcome: DeliveryOutcome::PeerMessageWritten,
                reachability: None,
                client: None,
            },
        );
        let engine = fixture.engine();
        assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
        drop(engine);
        let fixture = fixture.reopen().await;
        let original = fixture
            .owner
            .message_send_answer(&id)
            .await
            .unwrap()
            .unwrap()
            .result
            .unwrap();
        let identity = identity_without_native_work(
            fixture.owner.clone(),
            fixture.host.config.local.owner,
            automation.clone(),
        )
        .await;
        let app = (identity.clone()).build();
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
        drop(app);
        let mut no_delivery = identity.clone();
        no_delivery.mail_delivery = None;
        let app = (no_delivery).build();
        assert_eq!(
            app.messages()
                .message_send(input)
                .await
                .expect_err("no delivery")
                .kind,
            crate::collaboration_application::MessageFailureKind::Unavailable
        );
        assert!(
            fixture
                .owner
                .message_send_answer(&fresh)
                .await
                .expect("mail read")
                .is_none()
        );
        // The retired automation owner has no message tables to admit into.
        use sqlx::Connection as _;
        let mut observer = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(directory.path().join("automation.sqlite"))
                .read_only(true),
        )
        .await
        .expect("automation observer");
        let legacy_tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('router_pushes','mail_items','subscription_delivery_progress','board_intent_progress','board_intent_scan_cursors')")
            .fetch_one(&mut observer).await.expect("retired table inventory");
        observer.close().await.expect("close observer");
        assert_eq!(
            legacy_tables, 0,
            "message state returned to automation storage"
        );
        drop(app);
        drop(identity);
        drop(fixture.close().await);
        Arc::try_unwrap(automation)
            .unwrap_or_else(|_| panic!("mail store retained"))
            .into_inner()
            .close()
            .await
            .expect("close");
    }
}
