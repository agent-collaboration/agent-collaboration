//! Explicit Project claims traverse real native sessions and genuine takeover authority.
use super::{
    composed_fleet_fixture::*,
    fleet_claim_adapter_fixture::prepare_fleet,
    fleet_mail_dispatch::{Socket, envelope},
    fleet_mail_query::query,
    fleet_mail_settle::settle_answer,
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_delivery::Outbox;
use fleet_sync_protocol::{
    DatabaseId, DeliveryAnswer, DeliveryEnvelope, DeliveryFailureKind, DeliveryItem,
    DeliveryRefusal, DeliveryStatus, ItemId, ItemStatusState, SettleResult, StatusVersion,
    Submitter,
};
use serde_json::json;
use sqlx_core::{query::query as sql_query, row::Row};

fn item() -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    let DeliveryEnvelope::Item(source) = envelope(B, SERVICE_B)? else {
        return Err("item fixture returned a cancel".into());
    };
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        source.id(),
        serde_json::from_value(json!({"kind":"project","project":PROJECT}))?,
        source.recipient().clone(),
        None,
        source.body().clone(),
    )?))
}
fn claim() -> TestResult<Submitter> {
    Ok(Submitter::Project(DatabaseId::try_from(
        PROJECT.to_owned(),
    )?))
}
async fn owner(fixture: &Fixture, item: ItemId) -> TestResult<Option<Submitter>> {
    let limits = collaboration_central_core::fleet_outbox_limits()?;
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    limits,
                    &collaboration_central_core::FleetSenderBinding,
                )
                .relay_submitter(item)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)
            })
        })
        .await?)
}
async fn counts(fixture: &Fixture) -> TestResult<(i64, i64)> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=sql_query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM _fleet_sync_outbox_items) AS items,(SELECT count(*) FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.remove-record') AS removals")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("items").map_err(StoreError::backend)?,row.try_get("removals").map_err(StoreError::backend)?))
    })).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real takeover successor must settle unknown Project custody under its Project owner and Query the retained digest/status after owning reopen; the previous holder cannot authorize it."
)]
async fn genuine_successor_settles_unknown_project_and_query_keeps_its_identity_after_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), true).await?;
    let mut old = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut successor = Socket::open(&fixture, B, SERVICE_B).await?;
    let unknown = item()?;
    assert!(matches!(
        settle_answer(
            &fixture,
            &mut old,
            claim()?,
            unknown.id(),
            unknown.digest().clone()
        )
        .await?,
        DeliveryAnswer::MailboxUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    assert_eq!(owner(&fixture, unknown.id()).await?, None);
    assert!(
        matches!(settle_answer(&fixture,&mut successor,claim()?,unknown.id(),unknown.digest().clone()).await?,DeliveryAnswer::Settle(SettleResult::Settled {reason:DeliveryFailureKind::CustodyUnresolvable,version}) if version==StatusVersion::new(1))
    );
    assert_eq!(owner(&fixture, unknown.id()).await?, Some(claim()?));
    let DeliveryAnswer::Query(known) = query(&fixture, &mut successor, vec![unknown.id()]).await?
    else {
        return Err("successor Query lost its settled Project".into());
    };
    assert!(
        matches!(known.statuses.as_slice(),[status] if status.item==unknown.id() && matches!(&status.state,ItemStatusState::Known {digest,status:DeliveryStatus::Refused {reason:DeliveryRefusal::ProcessingFailed {kind:DeliveryFailureKind::CustodyUnresolvable}},version,cancel:None} if digest==unknown.digest() && *version==StatusVersion::new(1)))
    );
    assert!(matches!(
        query(&fixture, &mut old, vec![unknown.id()]).await?,
        DeliveryAnswer::MailboxUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    drop((old, successor));
    fixture.close().await?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut successor = Socket::open(&fixture, B, SERVICE_B).await?;
    assert_eq!(owner(&fixture, unknown.id()).await?, Some(claim()?));
    assert!(
        matches!(query(&fixture,&mut successor,vec![unknown.id()]).await?,DeliveryAnswer::Query(page) if page==known)
    );
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Normal frames carrying forged Machine or unauthorized Project claims must be refused before negative custody or removal work is admitted."
)]
async fn unauthorized_machine_and_project_claims_are_refused_before_any_custody_or_removal()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), true).await?;
    let mut old = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut unrelated = Socket::open(&fixture, C, SERVICE_C).await?;
    let unknown = item()?;
    let before = counts(&fixture).await?;
    for (peer, submitter) in [
        (A, claim()?),
        (C, claim()?),
        (A, Submitter::Machine(B.to_owned().try_into()?)),
        // Equal UUID bytes do not turn a machine's identity into a Project lease.
        (C, Submitter::Project(DatabaseId::try_from(C.to_owned())?)),
    ] {
        let socket = match peer {
            A => &mut old,
            C => &mut unrelated,
            _ => return Err("unknown native claim-case peer".into()),
        };
        assert!(matches!(
            settle_answer(
                &fixture,
                socket,
                submitter,
                unknown.id(),
                unknown.digest().clone()
            )
            .await?,
            DeliveryAnswer::MailboxUnavailable {
                reason: DeliveryRefusal::NotSubmitter
            }
        ));
        assert_eq!(owner(&fixture, unknown.id()).await?, None);
        assert_eq!(counts(&fixture).await?, before);
    }
    fixture.close().await
}
