use super::*;
use crate::{
    BoardAvailability, MailPushDeliveryProps, SubscriptionDeliveryService,
    SubscriptionDeliveryServiceProps, SystemSubscriptionClock,
};
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_contracts::MachineIdentity;
use collaboration_runtime::{MailDeliverySession, SessionDeliveryRouter};
use fleet_mail_delivery::{DeliveryLimits, minimum_settlement_allowance};
use fleet_protocol::{ClientAnswer, DeliveryRefusal, PeerId, SchemaSupport, ServerCallAnswer};
use fleet_session::{Endpoint, PeerCredential, SessionClient};
use fleet_session_client::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{sync::Arc, time::Duration};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup propagates errors."
)]
async fn full_push_inbox_returns_wire_capacity_refusal_without_admitting_custody() -> TestResult {
    // Arrange a real owner/engine with one byte of inbox room. No pump/model runs.
    let directory = tempfile::tempdir()?;
    let owner = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await?,
    );
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    let bound = Duration::from_secs(5);
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        ReqwestSessionTransport::new(
            AccessServiceToken::new("capacity-fixture", "capacity-fixture-secret")?,
            bound,
            bound,
        )?,
        MachineSessionStore::open(directory.path()).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: bound,
            max: bound,
            factor: 2,
        },
    )
    .await?;
    let (requests, _events, _pump) = SessionPump::new(
        client,
        Endpoint::new("ws://127.0.0.1:1/session")?,
        PeerCredential::new("Bearer capacity-fixture")?,
        bound,
    );
    let (_, welcomed) = tokio::sync::watch::channel(false);
    let session = MailDeliverySession {
        requests,
        welcomed,
        peer,
        relay: PeerId::new(uuid::Uuid::now_v7())?,
    };
    let service = uuid::Uuid::now_v7().to_string();
    let machine = MachineIdentity::new(service.clone().try_into()?, Some("push capacity"))?;
    let carrier = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: owner.clone(),
        delivery: carrier.clone(),
        presence: carrier.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(SystemSubscriptionClock),
    });
    let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
        8 * 1024 * 1024,
        100,
        1,
        minimum_settlement_allowance::<RouterDeliveryTypes>()?,
    )?;
    let runtime = MailDeliveryRuntime::with_limits(
        peer,
        MailPushDeliveryProps {
            interactions: owner.clone(),
            actors: actors.clone(),
            presence: carrier,
            machine,
            clock: Arc::new(SystemSubscriptionClock),
        },
        limits,
        Some(session),
    )?;
    let item = collaboration_runtime::outgoing_mail_message(
        &collaboration_protocol::PushRecordDraft {
            push_id: uuid::Uuid::now_v7().to_string().try_into()?,
            kind: collaboration_protocol::PushKind::DirectMessage,
            origin: collaboration_protocol::PushOrigin::OwnerUnverified,
            origin_router_ref: None,
            target: serde_json::from_value(
                serde_json::json!({"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"capacity-recipient"}),
            )?,
            mode: Some(collaboration_protocol::MessageDelivery::Auto),
            guard: None,
            reply_to_push_id: None,
            header_facts: collaboration_protocol::PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("capacity refusal retains the item at Fleet".to_owned()),
            activity: None,
            created_at: chrono::Utc::now(),
        },
        &service.try_into()?,
        peer,
    )?;
    let envelope: DeliveryEnvelope<BoardDeliveryTypes> =
        serde_json::from_value(serde_json::to_value(DeliveryEnvelope::Item(item))?)?;

    // Act through the same typed intake used by the machine socket.
    let answer = runtime
        .accept_pushed_mail(fleet_protocol::Submitter::Machine(peer), envelope)
        .await?;

    // Assert the wire refusal is not an error/receipt, and both projections stay absent.
    assert!(matches!(
        answer,
        PushResult::Refused {
            reason: DeliveryRefusal::MailboxFull
        }
    ));
    let encoded = serde_json::to_vec(&ClientAnswer::<BoardConsumerProtocol>::Crate(
        ServerCallAnswer::Pushed(answer),
    ))?;
    assert!(matches!(
        serde_json::from_slice::<ClientAnswer<BoardConsumerProtocol>>(&encoded)?,
        ClientAnswer::Crate(ServerCallAnswer::Pushed(PushResult::Refused {
            reason: DeliveryRefusal::MailboxFull
        }))
    ));
    let binding = runtime.binding.clone();
    let rows = runtime
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_mail_delivery::test_support::snapshot::<_, RouterDeliveryTypes, _>(
                    scope,
                    fleet_mail_delivery::DeliveryHostRole::Machine,
                    binding.as_ref(),
                )
                .await
                .map_err(StoreError::backend)
            })
        })
        .await?;
    assert!(!rows.iter().any(|row| row.category == "inbox"));
    drop(runtime);
    actors.shutdown().await;
    drop(actors);
    Arc::try_unwrap(owner)
        .map_err(|_| std::io::Error::other("capacity owner still held"))?
        .close()
        .await?;
    Ok(())
}
