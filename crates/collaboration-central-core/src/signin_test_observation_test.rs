//! The actual snapshot SQL and Rust decoder share the modern migrations.
use super::{SNAPSHOT, SignInObservation, decode_snapshot};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_storage::{ReadOutcome, SqlValue};
use fleet_identity_protocol::OrganizationId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx_core::{
    connection::ConnectOptions, executor::Executor, query::query, query_scalar::query_scalar,
    row::Row,
};
use sqlx_turso::{Turso, TursoConnection};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab01";
const OTHER_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab02";
const PERSON: &str = "019f0000-0000-7000-8000-00000000ab03";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000ab04";
const SESSION: &str = "019f0000-0000-7000-8000-00000000ab05";
const NOW: i64 = 1_800_000_000_000;
const TIME: &str = "2027-01-15T08:00:00Z";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";

#[tokio::test]
async fn computed_json_text_cast_preserves_the_runtime_text_type_through_a_cte() {
    use sqlx_core::{type_info::TypeInfo, value::ValueRef};
    let mut connection = empty_store().await;
    const DIRECT: &str =
        "SELECT json_object('value',1) AS payload,typeof(json_object('value',1)) AS runtime_type";
    const CTE: &str = "WITH payloads AS (SELECT json_object('value',1) AS payload) SELECT payload,typeof(payload) AS runtime_type FROM payloads";
    const CAST: &str = "WITH payloads AS (SELECT json_object('value',1) AS payload) SELECT CAST(payload AS TEXT) AS payload,typeof(payload) AS runtime_type FROM payloads";
    for (label, sql) in [("direct", DIRECT), ("cte", CTE), ("cast", CAST)] {
        let row = query::<Turso>(sql)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        let runtime_type: String = row.try_get("runtime_type").unwrap();
        let raw = row.try_get_raw("payload").unwrap();
        let reported_type = raw.type_info().name().to_owned();
        let unchecked_text: String = row.try_get_unchecked("payload").unwrap();
        let typed_text = row.try_get::<String, _>("payload");
        eprintln!(
            "computed JSON {label}: runtime={runtime_type}, driver={reported_type}, typed_text={}",
            typed_text.is_ok()
        );
        assert_eq!(runtime_type, "text");
        assert_eq!(unchecked_text, r#"{"value":1}"#);
        if label == "cast" {
            assert_eq!(reported_type, "TEXT");
            assert_eq!(typed_text.unwrap(), r#"{"value":1}"#);
        }
    }
}

fn organization() -> OrganizationId {
    ORGANIZATION.to_owned().try_into().unwrap()
}
fn fixture_id(suffix: usize) -> String {
    format!("019f0000-0000-7000-8000-{suffix:012x}")
}
fn statement() -> String {
    // Deliberately not a valid stamp profile: the observer reads bytes, not trust.
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"t","alg":"EdDSA","kid":"key-observed"}"#);
    let claims = URL_SAFE_NO_PAD.encode(br#"{"iat":1800000000,"exp":1800003600}"#);
    format!("{header}.{claims}.signature")
}
async fn empty_store() -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    for migration in [
        include_str!("../migrations/central/0001_workspace_requests.sql"),
        include_str!("../migrations/central/0002_mail_notices_timers.sql"),
        include_str!("../migrations/central/0003_signin_authority.sql"),
    ] {
        connection.execute(migration).await.unwrap();
    }
    connection
}
async fn stored_snapshot(connection: &mut TursoConnection) -> ReadOutcome {
    let rows = query::<Turso>(SNAPSHOT)
        .fetch_all(connection)
        .await
        .unwrap();
    ReadOutcome {
        columns: vec!["payload".into()],
        rows: rows
            .iter()
            .map(|row| vec![SqlValue::Text(row.try_get::<String, _>("payload").unwrap())])
            .collect(),
    }
}
async fn writes(connection: &mut TursoConnection) -> i64 {
    query_scalar::<Turso, i64>("SELECT total_changes()")
        .fetch_one(connection)
        .await
        .unwrap()
}
async fn stamped_store(records: usize) -> TursoConnection {
    let mut connection = empty_store().await;
    query::<Turso>("INSERT INTO organization_identity VALUES(?,?,?,?)")
        .bind(ORGANIZATION)
        .bind(statement())
        .bind(NOW)
        .bind(NOW)
        .execute(&mut connection)
        .await
        .unwrap();
    for index in 0..records {
        query::<Turso>("INSERT INTO organization_change_records VALUES(?,?,'organizationProvisioned','signInProvisioning',NULL,NULL,?,?,?)")
            .bind(fixture_id(0x1000+index)).bind(format!("request-{index}")).bind(ORGANIZATION).bind(NOW).bind(NOW).execute(&mut connection).await.unwrap();
    }
    query::<Turso>("INSERT INTO organization_authorization_floors VALUES(?,7,'active',?,0,?,?)")
        .bind(ORGANIZATION)
        .bind(POLICY)
        .bind(NOW)
        .bind(NOW)
        .execute(&mut connection)
        .await
        .unwrap();
    query::<Turso>(
        "INSERT INTO person_organization_authorization_floors VALUES(?,?,?,4,'active',?,?)",
    )
    .bind(SUBJECT)
    .bind(ORGANIZATION)
    .bind(PERSON)
    .bind(NOW)
    .bind(NOW)
    .execute(&mut connection)
    .await
    .unwrap();
    query::<Turso>(
        "INSERT INTO organization_used_requests VALUES(?,?,?,NULL,?,?,?,'thumbprint',?,?)",
    )
    .bind(fixture_id(0x3000))
    .bind(ORGANIZATION)
    .bind(NOW)
    .bind(PERSON)
    .bind(SESSION)
    .bind("intent-observed")
    .bind(NOW)
    .bind(NOW)
    .execute(&mut connection)
    .await
    .unwrap();
    query::<Turso>("INSERT INTO workspaces VALUES(?,'alpha','active',?,?)")
        .bind(fixture_id(0x9000))
        .bind(TIME)
        .bind(TIME)
        .execute(&mut connection)
        .await
        .unwrap();
    connection
}

#[test]
fn nothing_stored_matches_the_canonical_private_ipc_fixture() {
    // Arrange / Act: callers use this path without opening an unknown org store.
    let observed = serde_json::to_value(SignInObservation::nothing_stored()).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/signin-observation-empty.json"
    ))
    .unwrap();
    // Assert: this is empty, so assertion output contains no statement material.
    assert_eq!(observed, fixture);
}

#[tokio::test]
async fn one_snapshot_reads_all_modern_rows_and_writes_nothing() {
    // Arrange: exceed the historical page size, with every modern table represented.
    const RECORDS: usize = 250;
    let mut connection = stamped_store(RECORDS).await;
    let before = writes(&mut connection).await;
    // Act: the same one SELECT supplied by observe_signin_organization.
    let rows = stored_snapshot(&mut connection).await;
    let observed = decode_snapshot(&rows, &organization()).unwrap();
    // Assert: compare the raw compact only as an opaque boolean.
    assert_eq!(
        writes(&mut connection).await,
        before,
        "the observation wrote storage"
    );
    assert!(observed.complete);
    assert!(observed.statement.as_deref() == Some(statement().as_str()));
    let stamp = observed.stamp.as_ref().unwrap();
    assert_eq!(
        stamp.digest,
        format!("{:x}", Sha256::digest(statement().as_bytes()))
    );
    assert_eq!(stamp.key_id.as_deref(), Some("key-observed"));
    assert_eq!(stamp.expires_at, Some(1_800_003_600));
    assert_eq!(
        serde_json::to_value(stamp).unwrap(),
        json!({
            "digest":format!("{:x}",Sha256::digest(statement().as_bytes())),
            "keyId":"key-observed","expiresAt":1_800_003_600
        })
    );
    assert_eq!(
        observed
            .records
            .iter()
            .map(|record| record.request_id.as_str().to_owned())
            .collect::<Vec<_>>(),
        (0..RECORDS)
            .map(|index| format!("request-{index}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        observed.organization_floor.as_ref().unwrap().revision.get(),
        7
    );
    assert_eq!(observed.person_floors.len(), 1);
    assert_eq!(
        observed.person_floors.first().unwrap().person_id.as_str(),
        PERSON
    );
    assert_eq!(observed.used_requests.len(), 1);
    assert_eq!(
        observed.used_requests.first().unwrap().id.as_str(),
        fixture_id(0x3000)
    );
    assert_eq!(observed.workspaces.len(), 1);
    assert_eq!(observed.workspaces.first().unwrap().name.as_str(), "alpha");
    let records = serde_json::to_value(&observed.records).unwrap();
    assert_eq!(
        records.as_array().unwrap().first().unwrap(),
        &json!({"kind":"organizationProvisioned","requestId":"request-0","targetId":ORGANIZATION,"createdAtMillis":NOW})
    );
    assert_eq!(
        serde_json::to_value(&observed.used_requests).unwrap(),
        json!([{"id":fixture_id(0x3000)}])
    );
    assert_eq!(
        serde_json::to_value(&observed.workspaces).unwrap(),
        json!([{"workspaceId":fixture_id(0x9000),"name":"alpha"}])
    );
    assert_eq!(
        serde_json::to_value(&observed.organization_floor).unwrap(),
        json!({"revision":7,"state":"active"})
    );
    assert_eq!(
        serde_json::to_value(&observed.person_floors).unwrap(),
        json!([{
            "subjectId":SUBJECT,"personId":PERSON,"revision":4,"state":"active"
        }])
    );
}

#[tokio::test]
async fn empty_existing_store_is_a_complete_read_only_observation() {
    let mut connection = empty_store().await;
    let before = writes(&mut connection).await;
    let rows = stored_snapshot(&mut connection).await;
    let observed = decode_snapshot(&rows, &organization()).unwrap();
    assert!(observed == SignInObservation::nothing_stored());
    assert_eq!(writes(&mut connection).await, before);
}

#[tokio::test]
async fn malformed_stored_domains_and_foreign_binding_never_become_complete_observations() {
    for sql in [
        "UPDATE organization_authorization_floors SET minimum_revision=0",
        "UPDATE organization_authorization_floors SET state='paused'",
        "UPDATE person_organization_authorization_floors SET state='gone'",
        "UPDATE organization_used_requests SET actor_session_id='not-a-session'",
        "UPDATE workspaces SET state='archiving'",
        "UPDATE workspaces SET name=' alpha '",
    ] {
        let mut connection = stamped_store(1).await;
        connection.execute(sql).await.unwrap();
        let before = writes(&mut connection).await;
        let rows = stored_snapshot(&mut connection).await;
        assert!(
            decode_snapshot(&rows, &organization()).is_err(),
            "corrupt stored domain was accepted"
        );
        assert_eq!(
            writes(&mut connection).await,
            before,
            "even a refused observation must write nothing"
        );
    }
    let mut connection = stamped_store(1).await;
    let rows = stored_snapshot(&mut connection).await;
    let foreign: OrganizationId = OTHER_ORGANIZATION.to_owned().try_into().unwrap();
    assert!(
        decode_snapshot(&rows, &foreign).is_err(),
        "a snapshot must remain bound to its addressed org"
    );
}
