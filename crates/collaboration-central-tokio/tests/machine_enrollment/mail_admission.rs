//! A corrupt historical row is an unavailable owner, never a writer refusal.

use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use fleet_protocol::{DatabaseId, DeliveryEnvelope, DeliveryItem, Epoch, ItemId};
use serde_json::json;

/// A call as the shells deliver it: a kernel event, so what it owes is sent.
async fn call_event(fixture: &Fixture, body: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
    match collaboration_central_core::ObjectKernel::new(&fixture.runtime)
        .on_event(collaboration_central_core::ObjectEvent::Call {
            body,
            storage_bytes: None,
        })
        .await
    {
        collaboration_central_core::EventReply::CallAnswer(answer) => Ok(answer),
        collaboration_central_core::EventReply::CallFailure(failure) => Err(failure),
        _ => Err(CallFailure::AnswerLost),
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn corrupt_epoch_history_cannot_be_reported_as_not_writer() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let owner = owner()?;
    let origin = request(
        "550e8400-e29b-41d4-a716-4466554400b1",
        "mail-origin",
        "mail-origin-hash",
    )?;
    let recipient = request(
        "550e8400-e29b-41d4-a716-4466554400b2",
        "mail-recipient",
        "mail-recipient-hash",
    )?;
    let sender = peer(
        fixture
            .runtime
            .enroll_machine(&owner, origin.clone())
            .await?,
        &origin,
    )?;
    let receiving = peer(
        fixture
            .runtime
            .enroll_machine(&owner, recipient.clone())
            .await?,
        &recipient,
    )?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    fixture.object.transaction(move |scope| Box::pin(async move {
        // Deliberate on-disk corruption, outside production code. The query's
        // schema comes from the pinned public lease migration corpus.
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_epoch_history(id,database,epoch,holder,granted_at,created_at,updated_at) VALUES(?,?,'1','not a peer',1792281600000,1792281600000,1792281600000)")
            .bind(durable_steps::Uuid::now_v7().to_string()).bind(database.to_string()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    let corruption = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(fleet_project_lease::epoch_holder(
                    scope,
                    database,
                    Epoch::new(1).map_err(StoreError::backend)?,
                )
                .await)
            })
        })
        .await?;
    assert!(
        matches!(
            corruption,
            Err(fleet_project_lease::LeaseError::Corrupt { .. })
        ),
        "fixture must reach the real history decoder: {corruption:?}"
    );
    let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        serde_json::from_value(json!({"kind":"project","project":database}))?,
        serde_json::from_value(json!({"peer":receiving,"address":{
            "endpoint":{"serviceId":recipient.installation,"endpointId":"codex-local"},"sessionId":"recipient"
        }}))?,
        None,
        serde_json::from_value(json!({"kind":"notification","excerpt":"History corruption",
            "pointer":{"project":database,"subject":{"kind":"task","id":durable_steps::Uuid::now_v7()},"position":{"epoch":1,"position":1}},
            "source":{"kind":"intent","intent":durable_steps::Uuid::now_v7()}}))?,
    )?);
    let invocation = serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"session","authority":{"kind":"machine","value":{"installation":origin.installation}},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"mail_submit"},
        "kind":"central.mail-submit","target":null,"request":{
            "session":{"peer":sender,"sessionId":durable_steps::Uuid::now_v7()},
            "origin":{"installation":origin.installation,"machine":origin.label},"item":item,"originEpoch":1
        }
    }))?;
    let answer = call_event(&fixture, invocation.clone()).await;
    assert!(
        matches!(answer, Err(CallFailure::AnswerLost)),
        "corrupt history produced a MailSubmitAnswer: {answer:?}"
    );
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE _fleet_sync_epoch_history SET holder=? WHERE database=?",
                )
                .bind(sender.to_string())
                .bind(database.to_string())
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let repaired = call_event(&fixture, invocation)
        .await
        .map_err(|error| format!("repaired history did not reach Fleet admission: {error:?}"))?;
    let repaired: serde_json::Value = serde_json::from_slice(&repaired)?;
    assert_eq!(
        repaired.pointer("/value/value/kind"),
        Some(&json!("accepted")),
        "{repaired}"
    );
    fixture.objects.close().await?;
    Ok(())
}
