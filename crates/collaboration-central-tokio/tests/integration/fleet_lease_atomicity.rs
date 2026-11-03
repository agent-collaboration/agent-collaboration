//! Exercise the production Fleet renew entry, library hook and actual owning SQLite transaction.
#[expect(
    dead_code,
    reason = "The existing Fleet fixture also serves other integration targets."
)]
#[path = "../fleet_storage/support.rs"]
mod fleet_fixture;

use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_fixture::{Fixture, TestResult};
use fleet_sync_protocol::{
    DatabaseId, Epoch, LeaseAnswer, LeaseRequest, PeerId, RenewAnswer, RenewRequest, RequestId,
    SessionId,
};
use fleet_sync_session::SessionKey;
use serde_json::{Value, json};
use sqlx_core::{executor::Executor, query::query, row::Row};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const PEER: &str = "019f0000-0000-7000-8000-000000008111";
const SERVICE: &str = "019f0000-0000-7000-8000-000000008101";
const NOW: i64 = 1_792_281_600_000;

async fn seed_existing_grant(fixture: &Fixture) -> TestResult {
    fixture.import(&fleet_fixture::bundle()?).await?;
    fixture.object.transaction(|scope| Box::pin(async move {
        query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'writer','atomic-writer','atomic-credential','enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00')")
            .bind(PEER).bind(SERVICE).execute(scope.executor()).await.map_err(StoreError::backend)?;
        // Arrange only: a pre-existing library grant. The Act uses the public renew entry and its real hook.
        let database = json!({"database":PROJECT,"lifecycle":{"state":"registered","origin":{"kind":"empty"},"importedFrom":null},"revision":{"epoch":1,"change":1},"changed_at":NOW-1000});
        let lease = json!({"database":PROJECT,"holder":PEER,"epoch":1,"expires_at":NOW+1000,"last_renewed_at":NOW-1000});
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_databases(database,record) VALUES(?,?)").bind(PROJECT).bind(database.to_string()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO _fleet_sync_leases(database,record) VALUES(?,?)").bind(PROJECT).bind(lease.to_string()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(())
}
fn renew_call(attempt: RequestId) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"session","authority":{"kind":"machine","value":{"installation":SERVICE}},
        "destination":{"kind":"fleet"},"destinationFace":"registry",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"renew_atomicity"},
        "kind":"central.session-lease","target":null,"request":{
            "session":SessionKey {peer:PeerId::try_from(PEER.to_owned())?, session_id:SessionId::new(durable_steps::Uuid::now_v7())?},
            "request":LeaseRequest::Renew(RenewRequest {attempt,database:DatabaseId::try_from(PROJECT.to_owned())?,epoch:Epoch::new(1)?})
        }
    }))?)
}
async fn observe(fixture: &Fixture) -> TestResult<(Value, Option<String>, i64, i64)> {
    Ok(fixture.object.transaction(|scope| Box::pin(async move {
        let snapshot = fleet_sync_lease::registry_snapshot(scope, PROJECT.to_owned().try_into().map_err(StoreError::backend)?).await.map_err(StoreError::backend)?;
        let row = query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT count(*) FROM lease_incarnation) AS versions,(SELECT count(*) FROM machine_control_notices) AS notices FROM project_directory WHERE project_id=?")
            .bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((serde_json::to_value(snapshot).map_err(StoreError::backend)?,row.try_get("lease_projection").map_err(StoreError::backend)?,row.try_get("versions").map_err(StoreError::backend)?,row.try_get("notices").map_err(StoreError::backend)?))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare public renewal outcomes and persisted state; fixture errors propagate."
)]
#[tokio::test]
async fn renew_updates_the_directory_in_its_transaction_and_replay_adds_no_ownership_notice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    seed_existing_grant(&fixture).await?;
    let call = renew_call(RequestId::new(durable_steps::Uuid::now_v7())?)?;
    let answer = fixture
        .runtime
        .on_call(call.clone())
        .await
        .map_err(|failure| format!("renew: {failure:?}"))?;
    assert!(matches!(
        serde_json::from_slice::<LeaseAnswer>(&answer)?,
        LeaseAnswer::Renew(RenewAnswer::Renewed {
            replayed: false,
            ..
        })
    ));
    let first = observe(&fixture).await?;
    let projection: Value = serde_json::from_str(first.1.as_deref().ok_or("projection absent")?)?;
    assert_eq!(projection.get("snapshot"), Some(&first.0));
    assert_eq!(
        projection.pointer("/snapshot/grant/expiresAt"),
        Some(&json!(NOW + 60_000))
    );
    assert_eq!(projection.get("incarnation"), Some(&json!(1)));
    assert_eq!(
        (first.2, first.3),
        (1, 0),
        "same-holder renewal must not create ownership notices"
    );
    let replay = fixture
        .runtime
        .on_call(call)
        .await
        .map_err(|failure| format!("replay: {failure:?}"))?;
    assert!(matches!(
        serde_json::from_slice::<LeaseAnswer>(&replay)?,
        LeaseAnswer::Renew(RenewAnswer::Renewed { replayed: true, .. })
    ));
    assert_eq!(observe(&fixture).await?, first);
    // A fresh attempt at the same injected time leaves the lease revision unchanged as well.
    fixture
        .runtime
        .on_call(renew_call(RequestId::new(durable_steps::Uuid::now_v7())?)?)
        .await
        .map_err(|failure| format!("no-op: {failure:?}"))?;
    assert_eq!(observe(&fixture).await?, first);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The rollback assertion must fail when lease state escapes its directory transaction."
)]
#[tokio::test]
async fn a_directory_write_failure_rolls_back_the_real_library_renewal_and_retry_commits_once()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    seed_existing_grant(&fixture).await?;
    let before = observe(&fixture).await?;
    fixture.object.transaction(|scope| Box::pin(async move {
        scope.executor().execute("CREATE TRIGGER reject_projection BEFORE UPDATE OF lease_projection ON project_directory BEGIN SELECT RAISE(ABORT,'injected directory write failure'); END").await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    let call = renew_call(RequestId::new(durable_steps::Uuid::now_v7())?)?;
    assert!(fixture.runtime.on_call(call.clone()).await.is_err());
    assert_eq!(
        observe(&fixture).await?,
        before,
        "failed projection must roll back grant expiry, revision and incarnation"
    );
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                scope
                    .executor()
                    .execute("DROP TRIGGER reject_projection")
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let answer = fixture
        .runtime
        .on_call(call)
        .await
        .map_err(|failure| format!("retry: {failure:?}"))?;
    assert!(matches!(
        serde_json::from_slice::<LeaseAnswer>(&answer)?,
        LeaseAnswer::Renew(RenewAnswer::Renewed {
            replayed: false,
            ..
        })
    ));
    let after = observe(&fixture).await?;
    assert_eq!((after.2, after.3), (1, 0));
    assert_ne!(after.0, before.0);
    fixture.objects.close().await?;
    Ok(())
}
