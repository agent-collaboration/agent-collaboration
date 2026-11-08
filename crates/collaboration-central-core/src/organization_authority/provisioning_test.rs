use super::{ApplyProvisioning, ProvisioningApplied, apply};
use crate::organization_authority::{
    ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin,
    VerifiedFleetOrganizationStatement, change_records,
    fleet_statement_test_support::{
        ORGANIZATION, claims, header, signed_statement, verified_claims,
        verified_foreign_organization,
    },
};
use durable_steps::{Uuid, storage::TransactionScope};
use serde_json::json;
use sqlx_core::{connection::ConnectOptions, executor::Executor, query_as::query_as};
use sqlx_turso::{Turso, TursoConnection};

const NOW: i64 = 1_800_000_000_000;

struct FleetScope<'connection>(&'connection mut TursoConnection);
impl TransactionScope for FleetScope<'_> {
    type Database = Turso;
    type Executor<'query>
        = &'query mut TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

async fn fleet_store() -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    for migration in [
        include_str!("../../organization_migrations/0011_organization_identity.sql"),
        include_str!("../../organization_migrations/0012_organization_change_records.sql"),
    ] {
        connection.execute(migration).await.unwrap();
    }
    connection
}

fn record_id(suffix: u16) -> Uuid {
    Uuid::parse_str(&format!("019f0000-0000-7000-8000-00000000{suffix:04x}")).unwrap()
}

/// The applying transaction's clock, fixed for the case.
struct ApplyClock(i64);
impl runtime_primitives::Clock for ApplyClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(self.0)
    }
}

fn input(
    statement: VerifiedFleetOrganizationStatement,
    compact: &str,
    record: u16,
    now_millis: i64,
) -> ApplyProvisioning {
    ApplyProvisioning {
        statement,
        compact: compact.to_owned(),
        record_id: record_id(record),
        clock: std::sync::Arc::new(ApplyClock(now_millis)),
        stored_trusted: true,
        #[cfg(feature = "conformance")]
        fail_after_record: false,
    }
}

/// The exact stored identity row, so in-place overwrites and timestamps show.
async fn identity_rows(connection: &mut TursoConnection) -> Vec<(String, String, i64, i64)> {
    query_as("SELECT id, organization_statement, created_at, updated_at FROM organization_identity ORDER BY id")
        .fetch_all(connection)
        .await
        .unwrap()
}
type RecordRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
);
async fn record_rows(connection: &mut TursoConnection) -> Vec<RecordRow> {
    query_as("SELECT id, request_id, kind, origin, actor_person_id, permission_checked, target_id, created_at, updated_at FROM organization_change_records ORDER BY id")
        .fetch_all(connection)
        .await
        .unwrap()
}

fn first_compact() -> String {
    signed_statement(&header(), &claims())
}
async fn renewal_of(
    predecessor: &str,
    issued_at: i64,
) -> (VerifiedFleetOrganizationStatement, String) {
    use sha2::{Digest, Sha256};
    let mut input = claims();
    input["iat"] = json!(issued_at);
    input["replaces"] = json!(format!("{:x}", Sha256::digest(predecessor.as_bytes())));
    let compact = signed_statement(&header(), &input);
    (verified_claims(input).await, compact)
}

#[tokio::test(start_paused = true)]
async fn organization_provisioning_stamps_once_and_a_repeat_writes_nothing() {
    // Wrong build caught: a repeat that writes a second record or rewrites the
    // stamp (bytes or timestamps), or a setup record that invents a person.
    let mut connection = fleet_store().await;
    let compact = first_compact();

    let first = apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &compact, 1, NOW),
    )
    .await
    .unwrap();
    let identity = identity_rows(&mut connection).await;
    let records = record_rows(&mut connection).await;
    let repeat = apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &compact, 2, NOW + 5),
    )
    .await
    .unwrap();

    assert_eq!(first.applied, ProvisioningApplied::Provisioned);
    assert_eq!(repeat.applied, ProvisioningApplied::Provisioned);
    assert_eq!(
        identity,
        vec![(ORGANIZATION.into(), compact.clone(), NOW, NOW)]
    );
    assert_eq!(
        records,
        vec![(
            record_id(1).to_string(),
            ORGANIZATION.into(),
            "organizationProvisioned".into(),
            "signInProvisioning".into(),
            None,
            None,
            ORGANIZATION.into(),
            NOW,
            NOW,
        )]
    );
    assert_eq!(identity_rows(&mut connection).await, identity);
    assert_eq!(record_rows(&mut connection).await, records);
    let found = change_records::read_by_request(
        &mut FleetScope(&mut connection),
        &ChangeRequestId::try_from(ORGANIZATION.to_owned()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(found.kind, OrganizationChangeKind::OrganizationProvisioned);
    assert_eq!(
        found.origin,
        OrganizationChangeOrigin::SignInProvisioning {}
    );
}

#[tokio::test(start_paused = true)]
async fn organization_provisioning_never_hands_a_stamped_store_to_another_organization() {
    // Wrong build caught: an overwrite by B of A's stamp (the routing-bug model:
    // B's verified statement reaching A's store).
    let mut connection = fleet_store().await;
    let compact = first_compact();
    apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &compact, 1, NOW),
    )
    .await
    .unwrap();
    let identity = identity_rows(&mut connection).await;
    let records = record_rows(&mut connection).await;

    let foreign = apply(
        &mut FleetScope(&mut connection),
        input(verified_foreign_organization().await, "a.b.c", 2, NOW + 1),
    )
    .await
    .unwrap();

    assert_eq!(foreign.applied, ProvisioningApplied::OrganizationMismatch);
    assert_eq!(identity_rows(&mut connection).await, identity);
    assert_eq!(record_rows(&mut connection).await, records);
}

#[tokio::test(start_paused = true)]
async fn organization_renewal_swaps_only_the_named_predecessor_and_is_replay_safe() {
    // Wrong builds caught: a late statement prepared against the older stamp
    // replacing the newer one; a setup retry after renewal restoring the old
    // stamp; a lost-reply replay of the renewal refused or recorded twice.
    let mut connection = fleet_store().await;
    let original = first_compact();
    apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &original, 1, NOW),
    )
    .await
    .unwrap();
    let (renewal, renewal_compact) = renewal_of(&original, 1_799_999_950).await;
    let renewal_digest = renewal.digest().as_str().to_owned();
    let (late, late_compact) = renewal_of(&original, 1_799_999_951).await;
    let (replay, _) = renewal_of(&original, 1_799_999_950).await;

    let renewed = apply(
        &mut FleetScope(&mut connection),
        input(renewal, &renewal_compact, 2, NOW + 10),
    )
    .await
    .unwrap();
    let after_renewal = (
        identity_rows(&mut connection).await,
        record_rows(&mut connection).await,
    );
    let stale = apply(
        &mut FleetScope(&mut connection),
        input(late, &late_compact, 3, NOW + 20),
    )
    .await
    .unwrap();
    let setup_retry = apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &original, 4, NOW + 30),
    )
    .await
    .unwrap();
    let repeat = apply(
        &mut FleetScope(&mut connection),
        input(replay, &renewal_compact, 5, NOW + 40),
    )
    .await
    .unwrap();

    assert_eq!(renewed.applied, ProvisioningApplied::Provisioned);
    assert_eq!(stale.applied, ProvisioningApplied::StaleStatement);
    assert_eq!(setup_retry.applied, ProvisioningApplied::Provisioned);
    assert_eq!(repeat.applied, ProvisioningApplied::Provisioned);
    assert_eq!(
        after_renewal.0,
        vec![(ORGANIZATION.into(), renewal_compact.clone(), NOW, NOW + 10)]
    );
    let renewal_record = after_renewal.1.get(1).unwrap();
    assert_eq!(
        (
            renewal_record.1.as_str(),
            renewal_record.2.as_str(),
            renewal_record.7
        ),
        (renewal_digest.as_str(), "stampRenewed", NOW + 10)
    );
    assert_eq!(after_renewal.1.len(), 2);
    assert_eq!(identity_rows(&mut connection).await, after_renewal.0);
    assert_eq!(record_rows(&mut connection).await, after_renewal.1);
    // The typed page decoder reads back exactly what the raw rows hold.
    let page = change_records::read_page(&mut FleetScope(&mut connection), None, 10)
        .await
        .unwrap();
    assert_eq!(
        page.iter()
            .map(|record| (record.id.to_string(), record.kind))
            .collect::<Vec<_>>(),
        vec![
            (
                record_id(1).to_string(),
                OrganizationChangeKind::OrganizationProvisioned
            ),
            (
                record_id(2).to_string(),
                OrganizationChangeKind::StampRenewed
            ),
        ]
    );
}

#[cfg(feature = "conformance")]
#[tokio::test(start_paused = true)]
async fn organization_provisioning_rolls_the_stamp_back_with_a_failed_record() {
    // Wrong build caught: the stamp committed separately from its record.
    let mut connection = fleet_store().await;
    let compact = first_compact();
    connection.execute("BEGIN").await.unwrap();
    let mut failing = input(verified_claims(claims()).await, &compact, 1, NOW);
    failing.fail_after_record = true;

    let failed = apply(&mut FleetScope(&mut connection), failing).await;
    connection.execute("ROLLBACK").await.unwrap();
    let retried = apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &compact, 2, NOW + 1),
    )
    .await
    .unwrap();

    assert!(failed.is_err());
    assert_eq!(retried.applied, ProvisioningApplied::Provisioned);
    assert_eq!(identity_rows(&mut connection).await.len(), 1);
    let records = record_rows(&mut connection).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records.first().unwrap().0, record_id(2).to_string());
}

#[tokio::test(start_paused = true)]
async fn organization_provisioning_over_a_refused_stamp_accepts_only_its_replacement() {
    // Wrong build caught: a setup retry answering Provisioned over a stored
    // stamp that failed its check, so sign-in marks ready while central refuses.
    let mut connection = fleet_store().await;
    let original = first_compact();
    apply(
        &mut FleetScope(&mut connection),
        input(verified_claims(claims()).await, &original, 1, NOW),
    )
    .await
    .unwrap();
    let mut retry = input(verified_claims(claims()).await, &original, 2, NOW + 1);
    retry.stored_trusted = false;
    let (renewal, renewal_compact) = renewal_of(&original, 1_799_999_950).await;
    let mut replacement = input(renewal, &renewal_compact, 3, NOW + 2);
    replacement.stored_trusted = false;

    let refused = apply(&mut FleetScope(&mut connection), retry)
        .await
        .unwrap();
    let replaced = apply(&mut FleetScope(&mut connection), replacement)
        .await
        .unwrap();

    assert_eq!(refused.applied, ProvisioningApplied::StaleStatement);
    assert_eq!(replaced.applied, ProvisioningApplied::Provisioned);
    assert_eq!(
        identity_rows(&mut connection).await,
        vec![(ORGANIZATION.into(), renewal_compact, NOW, NOW + 2)]
    );
}
