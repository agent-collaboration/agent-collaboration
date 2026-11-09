//! Real Core sender callback, scoped lease authority and retained SDK settlement.
//! The already-held epoch-two lease is a supplied component input, not a takeover proof.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use durable_steps::{CallId, OperationId, StepName, StoreError};
use fleet_protocol::{
    DatabaseId, DeliveryFailureKind, DeliveryRefusal, DeliveryStatus, ItemDigest, ItemId,
    ItemStatusState, SessionId, SettleResult, StatusVersion, Submitter,
};
use fleet_tokio_objects::TokioObject;
use sqlx_core::{query::query, row::Row};

type Store = TokioObject<crate::CoreHost<FixtureIo>>;
type TestResult<T = ()> = Result<T, TestError>;
const A: &str = "019f0000-0000-7000-8000-000000008e11";
const B: &str = "019f0000-0000-7000-8000-000000008e12";
const C: &str = "019f0000-0000-7000-8000-000000008e13";
const SERVICE_A: &str = "019f0000-0000-7000-8000-000000008f11";
const SERVICE_B: &str = "019f0000-0000-7000-8000-000000008f12";
const SERVICE_C: &str = "019f0000-0000-7000-8000-000000008f13";
// Deliberately equal UUID bytes across namespaces; only the enum's tag grants authority.
const PROJECT: &str = B;

fn call() -> TestResult<CallId> {
    Ok(CallId {
        operation: OperationId::new(durable_steps::Uuid::now_v7())?,
        step: StepName::new("claim_settle")?,
    })
}
fn project() -> TestResult<Submitter> {
    Ok(Submitter::Project(DatabaseId::try_from(
        PROJECT.to_owned(),
    )?))
}

async fn prepare() -> TestResult<FleetStorageFixture> {
    let fixture = FleetStorageFixture::open().await?;
    for (peer, service) in [(A, SERVICE_A), (B, SERVICE_B), (C, SERVICE_C)] {
        fixture.enroll(peer.to_owned().try_into()?, service).await?;
    }
    fixture.object.transaction(|scope|Box::pin(async move {
        // Public SQLx migration apparatus has a version key of its own. Isolate
        // lease history from delivery history in this disposable test store.
        fleet_project_lease::LEASE_MIGRATOR.with_history_table("fixture_lease_history")
            .run_direct(None,scope.executor(),false).await.map_err(StoreError::backend)?;
        fleet_mail_delivery::DELIVERY_MIGRATOR.run_direct(None,scope.executor(),false).await.map_err(StoreError::backend)?;
        // Arrange an existing current B lease at epoch2. The callback below uses
        // public lease_status in this same real transaction; no publisher runs.
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_databases(database,lifecycle_kind,origin_kind,revision_epoch,revision_change,changed_at,created_at,updated_at) VALUES(?,'registered','empty','2','1',?,?,?)")
            .bind(PROJECT).bind(NOW.unix_millis()).bind(NOW.unix_millis()).bind(NOW.unix_millis()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_leases(database,holder,epoch,expires_at,last_renewed_at,created_at,updated_at) VALUES(?,?,'2',?,?,?,?)")
            .bind(PROJECT).bind(B).bind(NOW.unix_millis()+60_000).bind(NOW.unix_millis()).bind(NOW.unix_millis()).bind(NOW.unix_millis()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_epoch_floor(database,epoch,created_at,updated_at) VALUES(?,'2',?,?)")
            .bind(PROJECT).bind(NOW.unix_millis()).bind(NOW.unix_millis()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(fixture)
}

async fn answer(
    fixture: &FleetStorageFixture,
    peer: &str,
    service: &str,
    request: DeliveryRequest<BoardDeliveryTypes>,
    now: runtime_primitives::SyncTime,
) -> TestResult<DeliveryAnswer<BoardDeliveryTypes>> {
    let callback = AdmitSenderMail {
        source: service.to_owned().try_into()?,
        input: SenderMailRequest {
            session: SessionKey {
                peer: peer.to_owned().try_into()?,
                session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
            },
            request: Box::new(request),
        },
        limits: crate::fleet_outbox_limits()?,
        now,
    };
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let (route, _) = <AdmitSenderMail as StepCallback<Store, sqlx_turso::Turso>>::run(
                    callback, scope,
                )
                .await?;
                match route {
                    SenderRoute::Answered(answer) => Ok(answer),
                    SenderRoute::Refused => Err(StoreError::backend(
                        "supplied authenticated session did not match its independent enrollment",
                    )),
                }
            })
        })
        .await?)
}
fn settle(
    claim: Submitter,
    item: ItemId,
    digest: ItemDigest,
) -> TestResult<DeliveryRequest<BoardDeliveryTypes>> {
    Ok(DeliveryRequest::Settle {
        call: call()?,
        mailbox: C.to_owned().try_into()?,
        submitter: claim,
        item,
        digest,
    })
}
fn query_request(item: ItemId) -> TestResult<DeliveryRequest<BoardDeliveryTypes>> {
    Ok(DeliveryRequest::Query {
        call: call()?,
        mailbox: C.to_owned().try_into()?,
        items: vec![item],
    })
}
async fn counts(fixture: &FleetStorageFixture) -> TestResult<(i64, i64)> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM _fleet_sync_outbox_items) AS items,(SELECT count(*) FROM central_work_attempts) AS owed")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("items").map_err(StoreError::backend)?,row.try_get("owed").map_err(StoreError::backend)?))
    })).await?)
}

async fn recorded_owner(
    fixture: &FleetStorageFixture,
    item: ItemId,
) -> TestResult<Option<Submitter>> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    crate::fleet_outbox_limits().map_err(StoreError::backend)?,
                    &crate::fleet_sender_binding::FleetSenderBinding,
                )
                .relay_submitter(item)
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)
            })
        })
        .await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Current holder B must settle an unknown Project record under Project ownership and recover its exact digest/status after owning reopen; the old holder and same-UUID Machine claim cannot steal it."
)]
async fn current_holder_settles_unknown_project_and_query_preserves_its_owner_after_reopen()
-> TestResult {
    let fixture = prepare().await?;
    let item = ItemId::new(durable_steps::Uuid::now_v7())?;
    let digest = ItemDigest::try_from("a".repeat(64))?;
    let response = answer(
        &fixture,
        B,
        SERVICE_B,
        settle(project()?, item, digest.clone())?,
        NOW,
    )
    .await?;
    assert!(
        matches!(response,DeliveryAnswer::Settle(SettleResult::Settled {reason:DeliveryFailureKind::CustodyUnresolvable,version}) if version==StatusVersion::new(1))
    );
    assert_eq!(recorded_owner(&fixture, item).await?, Some(project()?));
    let checked = answer(&fixture, B, SERVICE_B, query_request(item)?, NOW).await?;
    let DeliveryAnswer::Query(known) = checked else {
        return Err("Project holder Query was not answered".into());
    };
    assert!(
        matches!(known.statuses.as_slice(),[status] if status.item==item && matches!(&status.state,ItemStatusState::Known {digest:stored,status:DeliveryStatus::Refused {reason:DeliveryRefusal::ProcessingFailed {kind:DeliveryFailureKind::CustodyUnresolvable}},version,cancel:None} if stored==&digest && *version==StatusVersion::new(1)))
    );
    let baseline = counts(&fixture).await?;
    // Settled custody, and no owed row: the mail retention due source removes
    // the record when its retention ends.
    assert_eq!(baseline, (1, 0));
    for (peer, service) in [(A, SERVICE_A), (C, SERVICE_C)] {
        assert!(matches!(
            answer(&fixture, peer, service, query_request(item)?, NOW).await?,
            DeliveryAnswer::DeliveryUnavailable {
                reason: DeliveryRefusal::NotSubmitter
            }
        ));
    }
    // B is also a legitimate machine caller, but Machine(B) and Project(B)
    // remain different owners even when their UUID bytes and digest match.
    assert!(
        matches!(answer(&fixture,B,SERVICE_B,settle(Submitter::Machine(B.to_owned().try_into()?),item,digest.clone())?,NOW).await?,DeliveryAnswer::Settle(SettleResult::ItemConflict {existing}) if existing==digest)
    );
    assert_eq!(counts(&fixture).await?, baseline);
    let fixture = fixture.reopen().await?;
    assert_eq!(recorded_owner(&fixture, item).await?, Some(project()?));
    let after = answer(&fixture, B, SERVICE_B, query_request(item)?, NOW).await?;
    assert!(matches!(after,DeliveryAnswer::Query(page) if page==known));
    assert_eq!(counts(&fixture).await?, baseline);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Forged machine, absent/expired Project and old-holder claims must be refused before any custody or owed work is admitted, while unknown Query stays source-free."
)]
async fn unauthorized_settlement_claims_never_create_negative_custody_or_owed_work() -> TestResult {
    let fixture = prepare().await?;
    let item = ItemId::new(durable_steps::Uuid::now_v7())?;
    let digest = ItemDigest::try_from("b".repeat(64))?;
    let baseline = counts(&fixture).await?;
    for (peer, service, claim, now) in [
        (A, SERVICE_A, project()?, NOW),
        (C, SERVICE_C, project()?, NOW),
        (
            B,
            SERVICE_B,
            Submitter::Machine(A.to_owned().try_into()?),
            NOW,
        ),
        (
            A,
            SERVICE_A,
            Submitter::Project(DatabaseId::try_from(A.to_owned())?),
            NOW,
        ),
        (
            B,
            SERVICE_B,
            project()?,
            NOW.after(std::time::Duration::from_secs(60)),
        ),
    ] {
        assert!(matches!(
            answer(
                &fixture,
                peer,
                service,
                settle(claim, item, digest.clone())?,
                now
            )
            .await?,
            DeliveryAnswer::DeliveryUnavailable {
                reason: DeliveryRefusal::NotSubmitter
            }
        ));
        assert_eq!(counts(&fixture).await?, baseline);
    }
    assert!(
        matches!(answer(&fixture,B,SERVICE_B,query_request(item)?,NOW).await?,DeliveryAnswer::Query(page) if matches!(page.statuses.as_slice(),[status] if status.item==item && matches!(status.state,ItemStatusState::Unknown)))
    );
    assert_eq!(counts(&fixture).await?, baseline);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The real claim owner and sender callback must refuse Fleet authority even when its UUID equals the enrolled peer."
)]
async fn enrolled_machine_cannot_settle_as_fleet_with_matching_uuid() -> TestResult {
    let fixture = prepare().await?;
    let peer = B.to_owned().try_into()?;
    let claim = Submitter::Fleet(B.to_owned().try_into()?);
    let authorized = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::mail_sender_authority::claim_is_authorized(scope, peer, claim, NOW).await
            })
        })
        .await?;
    assert!(
        !authorized,
        "machine enrollment granted Fleet claim authority"
    );
    let baseline = counts(&fixture).await?;
    let item = ItemId::new(durable_steps::Uuid::now_v7())?;
    let digest = ItemDigest::try_from("c".repeat(64))?;
    assert!(matches!(
        answer(&fixture, B, SERVICE_B, settle(claim, item, digest)?, NOW).await?,
        DeliveryAnswer::DeliveryUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    assert_eq!(
        counts(&fixture).await?,
        baseline,
        "refusal created custody or owed work"
    );
    fixture.close().await
}
