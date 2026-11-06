//! Independent enrollment rows, real SDK custody and confirmed transaction rollback.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use board_sync_protocol::BoardDeliveryBody;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_delivery::{CustodyRow, DeliveryTransactionFailure, Outbox};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryItem, DeliveryItemRef, DeliveryStatus, ItemId, PeerId, Recipient,
    SubmitResult,
};
use fleet_sync_tokio_runner::TokioObject;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

const SERVICE_A: &str = "019f0000-0000-7000-8000-000000008741";
const SERVICE_B: &str = "019f0000-0000-7000-8000-000000008742";
type Store = TokioObject<crate::CoreHost<FixtureIo>>;
type Push = crate::fleet_mail_tasks::FleetPushTask<Store, sqlx_turso::Turso>;
type Removal = crate::fleet_mail_tasks::FleetMailRemoval<Store, sqlx_turso::Turso>;
type TestResult<T = ()> = Result<T, TestError>;

async fn lookup(
    fixture: &FleetStorageFixture,
    sender: MailSender,
) -> TestResult<Result<Submitter, SenderBindingFailure>> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { Ok(FleetSenderBinding.resolve_submitter(scope, &sender).await) })
        })
        .await?)
}

fn envelope(
    sender: MailSender,
    recipient: PeerId,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    let body: BoardDeliveryBody = serde_json::from_value(
        json!({"kind":"message","text":"sealed sender stays unchanged","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
    )?;
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        sender,
        Recipient {
            peer: recipient,
            address: serde_json::from_value(
                json!({"endpoint":{"serviceId":SERVICE_B,"endpointId":"codex-local"},"sessionId":"binding-recipient"}),
            )?,
        },
        None,
        body,
    )?))
}

async fn prepare_custody() -> TestResult<(FleetStorageFixture, PeerId, PeerId)> {
    let fixture = FleetStorageFixture::open().await?;
    let sender = PeerId::new(durable_steps::Uuid::now_v7())?;
    let recipient = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture.enroll(sender, SERVICE_A).await?;
    fixture.enroll(recipient, SERVICE_B).await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::DELIVERY_MIGRATOR
                    .run_direct(None, scope.executor(), false)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    Ok((fixture, sender, recipient))
}

async fn submit(
    fixture: &FleetStorageFixture,
    submitter: Submitter,
    item: DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult<Value> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let answer = Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                    &FleetSenderBinding,
                )
                .submit_relay::<Push, Removal>(
                    submitter,
                    item,
                    "019f0000-0000-7000-8000-000000005240"
                        .to_owned()
                        .try_into()
                        .map_err(StoreError::backend)?,
                    SERVICE_A
                        .to_owned()
                        .try_into()
                        .map_err(StoreError::backend)?,
                    NOW,
                )
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)?;
                serde_json::to_value(answer).map_err(StoreError::backend)
            })
        })
        .await?)
}

async fn counts(fixture: &FleetStorageFixture) -> TestResult<(i64, i64)> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM _fleet_sync_outbox_items) AS custody,(SELECT count(*) FROM _fleet_sync_operations) AS tasks")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("custody").map_err(StoreError::backend)?,row.try_get("tasks").map_err(StoreError::backend)?))
    })).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The expected peers come from independent enrollment inputs, never the sealed sender UUID bits."
)]
async fn every_sender_variant_resolves_without_changing_its_source_bytes_after_reopen() -> TestResult
{
    let fixture = FleetStorageFixture::open().await?;
    let first = PeerId::new(durable_steps::Uuid::now_v7())?;
    let second = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture.enroll(first, SERVICE_A).await?;
    fixture.enroll(second, SERVICE_B).await?;
    let fixture = fixture.reopen().await?;
    let project = fleet_sync_protocol::DatabaseId::new(durable_steps::Uuid::now_v7())?;
    for (sender, expected) in [
        (
            serde_json::from_value::<MailSender>(
                json!({"kind":"session","endpoint":{"serviceId":SERVICE_A,"endpointId":"codex-local"},"sessionId":"original-session"}),
            )?,
            Submitter::Machine(first),
        ),
        (
            MailSender::HumanUser {
                installation: SERVICE_B.to_owned().try_into()?,
            },
            Submitter::Machine(second),
        ),
        (
            MailSender::Project {
                project: project.to_string().try_into()?,
            },
            Submitter::Project(project),
        ),
    ] {
        let sealed = serde_json::to_vec(&sender)?;
        assert_eq!(lookup(&fixture, sender.clone()).await?, Ok(expected));
        assert_eq!(serde_json::to_vec(&sender)?, sealed);
    }
    assert_ne!(first.as_uuid().to_string(), SERVICE_A);
    assert_ne!(second.as_uuid().to_string(), SERVICE_B);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Missing and malformed independent source rows are binding failures, never guessed owners or custody corruption."
)]
async fn missing_or_malformed_enrollment_never_supplies_a_guessed_machine() -> TestResult {
    let fixture = FleetStorageFixture::open().await?;
    let sender = MailSender::HumanUser {
        installation: SERVICE_A.to_owned().try_into()?,
    };
    assert_eq!(
        lookup(&fixture, sender.clone()).await?,
        Err(SenderBindingFailure::Unknown)
    );
    fixture
        .enroll(PeerId::new(durable_steps::Uuid::now_v7())?, SERVICE_A)
        .await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE machine_enrollments SET credential_hash=' ' WHERE service_id=?",
                )
                .bind(SERVICE_A)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    assert_eq!(
        lookup(&fixture, sender).await?,
        Err(SenderBindingFailure::Unavailable)
    );
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A false authenticated claim or missing binding must commit zero custody and work; the true claim preserves exact sealed bytes."
)]
async fn admission_compares_independent_binding_and_claim_before_any_custody_or_work() -> TestResult
{
    let (fixture, sender, recipient) = prepare_custody().await?;
    let item = envelope(
        MailSender::HumanUser {
            installation: SERVICE_A.to_owned().try_into()?,
        },
        recipient,
    )?;
    let original = serde_json::to_vec(&item)?;
    assert_eq!(
        submit(
            &fixture,
            Submitter::Machine(recipient),
            serde_json::from_slice(&original)?
        )
        .await?,
        json!({"kind":"refused","value":{"reason":{"kind":"notSubmitter"}}})
    );
    assert_eq!(counts(&fixture).await?, (0, 0));
    let unknown = envelope(
        MailSender::HumanUser {
            installation: "019f0000-0000-7000-8000-000000008743"
                .to_owned()
                .try_into()?,
        },
        recipient,
    )?;
    assert_eq!(
        submit(&fixture, Submitter::Machine(sender), unknown).await?,
        json!({"kind":"bindingUnavailable","value":{"reason":"unknown"}})
    );
    assert_eq!(counts(&fixture).await?, (0, 0));
    let reference = DeliveryItemRef {
        item: item.id(),
        digest: item.digest().clone(),
    };
    let accepted = submit(&fixture, Submitter::Machine(sender), item).await?;
    assert!(matches!(
        serde_json::from_value::<SubmitResult<collaboration_mail_protocol::CarrierAcceptance>>(
            accepted
        )?,
        SubmitResult::Accepted {
            status: DeliveryStatus::Sent,
            ..
        }
    ));
    let resolved = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                    &FleetSenderBinding,
                )
                .resolve_push(&reference, NOW)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)
            })
        })
        .await?
        .ok_or("real queued Push disappeared")?;
    assert_eq!(resolved.submitter, Submitter::Machine(sender));
    assert_eq!(serde_json::to_vec(&resolved.item)?, original);
    assert_eq!(counts(&fixture).await?, (1, 2));
    fixture.close().await
}

async fn rolled_back_read(fixture: &FleetStorageFixture, item: ItemId) -> StoreError {
    let result = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE machine_enrollments SET credential_hash=' ' WHERE service_id=?",
                )
                .bind(SERVICE_A)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                    &FleetSenderBinding,
                )
                .status(item)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)
            })
        })
        .await;
    match result {
        Err(error) => error,
        Ok(_) => panic!("malformed independent sender row was accepted"),
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The actual failed custody read must roll back its source fault and stay retryable without changing body or work."
)]
async fn confirmed_binding_rollback_is_retryable_and_retains_custody_and_original_source()
-> TestResult {
    let (fixture, sender, recipient) = prepare_custody().await?;
    let item = envelope(
        MailSender::HumanUser {
            installation: SERVICE_A.to_owned().try_into()?,
        },
        recipient,
    )?;
    let id = item.id();
    let original = serde_json::to_vec(&item)?;
    let reference = DeliveryItemRef {
        item: id,
        digest: item.digest().clone(),
    };
    submit(&fixture, Submitter::Machine(sender), item).await?;
    let before = counts(&fixture).await?;
    assert!(
        matches!(fleet_sync_delivery::classify_delivery_store_error(rolled_back_read(&fixture,id).await),
        DeliveryTransactionFailure::BindingUnavailable {row:CustodyRow::Outbox(actual),reason:SenderBindingFailure::Unavailable} if actual==id)
    );
    let failure = crate::mail_push_dispatch::store_failure(rolled_back_read(&fixture, id).await);
    assert!(
        matches!(failure,durable_steps::CallFailure::Retryable {reason} if reason.code=="senderBindingUnavailable")
    );
    assert_eq!(
        lookup(
            &fixture,
            MailSender::HumanUser {
                installation: SERVICE_A.to_owned().try_into()?
            }
        )
        .await?,
        Ok(Submitter::Machine(sender))
    );
    assert_eq!(counts(&fixture).await?, before);
    let resolved = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                    &FleetSenderBinding,
                )
                .resolve_push(&reference, NOW)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)
            })
        })
        .await?
        .ok_or("binding rollback lost queued content")?;
    assert_eq!(resolved.submitter, Submitter::Machine(sender));
    assert_eq!(serde_json::to_vec(&resolved.item)?, original);
    assert!(matches!(
        crate::mail_push_dispatch::store_failure(StoreError::backend(
            "unconfirmed transaction outcome"
        )),
        durable_steps::CallFailure::AnswerLost
    ));
    fixture.close().await
}
