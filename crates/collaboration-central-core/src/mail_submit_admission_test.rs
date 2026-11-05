//! The admission transaction checks current enrollment without losing historical binding.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, SessionId};
use fleet_sync_session::SessionKey;
use sqlx_core::{query::query, row::Row};

const SENDER: &str = "019f0000-0000-7000-8000-000000008f21";
const RECIPIENT: &str = "019f0000-0000-7000-8000-000000008f22";
const SERVICE: &str = "019f0000-0000-7000-8000-000000008f23";
const RECIPIENT_SERVICE: &str = "019f0000-0000-7000-8000-000000008f24";
type FixtureStore = fleet_sync_tokio_runner::TokioObject<crate::CoreHost<FixtureIo>>;

#[tokio::test]
async fn enrollment_expiry_refuses_at_its_exact_boundary_and_preserves_historical_binding()
-> Result<(), TestError> {
    let fixture = FleetStorageFixture::open().await?;
    fixture
        .enroll(SENDER.to_owned().try_into()?, SERVICE)
        .await?;
    fixture
        .enroll(RECIPIENT.to_owned().try_into()?, RECIPIENT_SERVICE)
        .await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::DELIVERY_MIGRATOR
                    .run_direct(None, scope.executor(), false)
                    .await
                    .map_err(StoreError::backend)?;
                crate::generated_enrollment::update_enrollment(
                    scope,
                    "expired sender".into(),
                    "expired-client".into(),
                    "expired-hash".into(),
                    chrono::DateTime::from_timestamp_millis(NOW.unix_millis())
                        .ok_or_else(|| StoreError::backend("fixture clock"))?
                        .to_rfc3339(),
                    "2026-10-18T00:00:00+00:00".into(),
                    SENDER.into(),
                )
                .await
                .map_err(StoreError::backend)
            })
        })
        .await?;
    for now in [NOW, NOW.after(std::time::Duration::from_millis(1))] {
        let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(durable_steps::Uuid::now_v7())?,
            serde_json::from_value(serde_json::json!({"kind":"humanUser","installation":SERVICE}))?,
            serde_json::from_value(serde_json::json!({"peer":RECIPIENT,"address":{
                "endpoint":{"serviceId":RECIPIENT_SERVICE,"endpointId":"codex-local"},"sessionId":"expiry-recipient"}}))?,
            None,
            serde_json::from_value(serde_json::json!({"kind":"message","text":"expired sender",
                "delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
        )?);
        let callback = AdmitMailSubmit {
            source: SERVICE.to_owned().try_into()?,
            request: MailSubmitRequest {
                session: SessionKey {
                    peer: SENDER.to_owned().try_into()?,
                    session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
                },
                origin: crate::AdmissionOrigin {
                    installation: SERVICE.to_owned().try_into()?,
                    machine: "expired sender".to_owned().try_into()?,
                },
                item: Box::new(item),
                origin_epoch: None,
            },
            limits: crate::fleet_outbox_limits()?,
            relay: PeerId::new(durable_steps::Uuid::now_v7())?,
            clock: std::sync::Arc::new(crate::object_bootstrap::FixedBootstrapClock(now)),
        };
        fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let (reply, _) = <AdmitMailSubmit as StepCallback<
                        FixtureStore,
                        sqlx_turso::Turso,
                    >>::run(callback, scope)
                    .await?;
                    if !matches!(
                        reply,
                        MailSubmitReply::Answer(MailSubmitAnswer::Submitted(
                            SubmitResult::Refused {
                                reason: DeliveryRefusal::SenderNotEnrolled
                            }
                        ))
                    ) {
                        return Err(StoreError::backend(
                            "expired sender was not refused with SenderNotEnrolled",
                        ));
                    }
                    let count: i64 = query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS count FROM _fleet_sync_outbox_items",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("count")
                    .map_err(StoreError::backend)?;
                    if count != 0 {
                        return Err(StoreError::backend("expired sender created custody"));
                    }
                    let historical = crate::registry_peer_lookup::service_for_peer(
                        scope,
                        SENDER.to_owned().try_into().map_err(StoreError::backend)?,
                    )
                    .await?;
                    if historical.as_ref().map(ServiceId::as_str) != Some(SERVICE) {
                        return Err(StoreError::backend(
                            "expiry erased historical sender identity",
                        ));
                    }
                    Ok(())
                })
            })
            .await?;
    }
    fixture.close().await
}
