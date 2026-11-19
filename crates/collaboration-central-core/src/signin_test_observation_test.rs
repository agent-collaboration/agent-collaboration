//! The actual snapshot SQL and Rust decoder share the modern migrations.
use super::{SNAPSHOT, SignInObservation, decode_snapshot};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use central_storage::{ReadOutcome, SqlValue};
use fleet_identity_protocol::OrganizationId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqliteConnection};
use sqlx_core::{
    connection::ConnectOptions, executor::Executor, query::query, query_scalar::query_scalar,
    row::Row,
};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab01";
const OTHER_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab02";
const PERSON: &str = "019f0000-0000-7000-8000-00000000ab03";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000ab04";
const SESSION: &str = "019f0000-0000-7000-8000-00000000ab05";
const NOW: i64 = 1_800_000_000_000;
const TIME: &str = "2027-01-15T08:00:00Z";
// switchboard@b1c9f6f6: src/organizations/fleet-permission-policy-digest.ts
const POLICY: &str = "af40ec7fb999c4ef7f9611f322bab013e05666b56cd1d3101fac1e9e001d074a";

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
async fn empty_store() -> SqliteConnection {
    let mut connection = "sqlite::memory:"
        .parse::<SqliteConnectOptions>()
        .unwrap()
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
async fn stored_snapshot(connection: &mut SqliteConnection) -> ReadOutcome {
    let rows = query::<Sqlite>(SNAPSHOT)
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
async fn writes(connection: &mut SqliteConnection) -> i64 {
    query_scalar::<Sqlite, i64>("SELECT total_changes()")
        .fetch_one(connection)
        .await
        .unwrap()
}
async fn stamped_store(records: usize) -> SqliteConnection {
    let mut connection = empty_store().await;
    query::<Sqlite>("INSERT INTO signin_organization_identity VALUES(?,?,?,?)")
        .bind(ORGANIZATION)
        .bind(statement())
        .bind(NOW)
        .bind(NOW)
        .execute(&mut connection)
        .await
        .unwrap();
    for index in 0..records {
        query::<Sqlite>("INSERT INTO signin_change_records VALUES(?,?,'organizationProvisioned','signInProvisioning',NULL,NULL,?,?,?)")
            .bind(fixture_id(0x1000+index)).bind(format!("request-{index}")).bind(ORGANIZATION).bind(NOW).bind(NOW).execute(&mut connection).await.unwrap();
    }
    query::<Sqlite>("INSERT INTO signin_authorization_floors VALUES(?,7,'active',?,0,?,?)")
        .bind(ORGANIZATION)
        .bind(POLICY)
        .bind(NOW)
        .bind(NOW)
        .execute(&mut connection)
        .await
        .unwrap();
    query::<Sqlite>("INSERT INTO signin_person_authorization_floors VALUES(?,?,?,4,'active',?,?)")
        .bind(SUBJECT)
        .bind(ORGANIZATION)
        .bind(PERSON)
        .bind(NOW)
        .bind(NOW)
        .execute(&mut connection)
        .await
        .unwrap();
    query::<Sqlite>("INSERT INTO admin_used_requests VALUES(?,?,?,NULL,?,?,?,'thumbprint',?,?)")
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
    query::<Sqlite>("INSERT INTO directory_workspaces(workspace_id,name,state,created_at,updated_at) VALUES(?,'alpha','active',?,?)")
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
    let fixture: Value = serde_json::from_str(super::EMPTY_SIGNIN_OBSERVATION_JSON).unwrap();
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
        "UPDATE signin_authorization_floors SET minimum_revision=0",
        "UPDATE signin_authorization_floors SET state='paused'",
        "UPDATE signin_person_authorization_floors SET state='gone'",
        "UPDATE admin_used_requests SET actor_session_id='not-a-session'",
        "UPDATE directory_workspaces SET state='archiving'",
        "UPDATE directory_workspaces SET name=' alpha '",
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

async fn machine_store() -> SqliteConnection {
    let mut connection = stamped_store(1).await;
    query::<Sqlite>("INSERT INTO login_machines VALUES(?,'Observed machine','enrolled',?,7,?,?)")
        .bind(fixture_id(0xe000))
        .bind(serde_json::to_string(&vec![fixture_id(0x9000)]).unwrap())
        .bind(TIME)
        .bind(TIME)
        .execute(&mut connection)
        .await
        .unwrap();
    query::<Sqlite>(
        "INSERT INTO login_machines VALUES(?,'Second observed machine','revoked','[]',0,?,?)",
    )
    .bind(fixture_id(0xe040))
    .bind(TIME)
    .bind(TIME)
    .execute(&mut connection)
    .await
    .unwrap();
    // Insert reverse identifiers and creation times so ID order is the wrong oracle.
    for (index, created, state, reason, client, last, ended) in [
        (
            2,
            "2027-01-15T08:00:02Z",
            "approved",
            None,
            None,
            None,
            None,
        ),
        (
            1,
            "2027-01-15T08:00:01Z",
            "enrolled",
            None,
            Some(fixture_id(0xe031)),
            Some(TIME),
            None,
        ),
        (
            3,
            "2027-01-15T08:00:00Z",
            "ended",
            Some("idle"),
            Some(fixture_id(0xe033)),
            None,
            Some(TIME),
        ),
    ] {
        query::<Sqlite>("INSERT INTO login_machine_approvals VALUES(?,?,?,'Approval snapshot','[]',?,?,?,?,7,?,?,?,?,?,?)")
            .bind(fixture_id(0xe010 + index)).bind(fixture_id(0xe020 + index)).bind(fixture_id(0xe000))
            .bind(state).bind(reason).bind(PERSON).bind("machine:enroll")
            .bind("2027-01-15T08:10:00+00:00").bind(client).bind(last).bind(ended).bind(created).bind(created)
            .execute(&mut connection).await.unwrap();
    }
    connection
}

#[tokio::test]
async fn machine_snapshot_exports_checked_lifecycle_fields_in_creation_order() {
    // Arrange: real migrated SQLite rows, with different machine/approval allowances.
    let mut connection = machine_store().await;
    let before = writes(&mut connection).await;
    // Act: the actual one-statement observation and its public fixture encoding.
    let observed =
        decode_snapshot(&stored_snapshot(&mut connection).await, &organization()).unwrap();
    let value = serde_json::to_value(&observed).unwrap();
    // Assert: all fields, exact nulls, canonical times and creation-order semantics.
    let mut machines = value["machines"].as_array().unwrap().clone();
    machines.sort_by(|left, right| left["machineId"].as_str().cmp(&right["machineId"].as_str()));
    assert_eq!(
        Value::Array(machines),
        json!([{
            "machineId":fixture_id(0xe000),
            "state":"enrolled","allowedWorkspaces":[fixture_id(0x9000)],"enrollmentGeneration":7
        },{
            "machineId":fixture_id(0xe040),
            "state":"revoked","allowedWorkspaces":[],"enrollmentGeneration":0
        }])
    );
    assert_eq!(
        value["machineApprovals"],
        json!([
            {"approvalId":fixture_id(0xe013),"requestId":fixture_id(0xe023),"machineId":fixture_id(0xe000),
             "state":"ended","endReason":"idle","allowedWorkspaces":[],"baseGeneration":7,
             "codeExpiresAt":"2027-01-15T08:10:00.000Z","machineClientId":fixture_id(0xe033),
             "lastGrantAt":null,"endedAt":"2027-01-15T08:00:00.000Z","createdAt":"2027-01-15T08:00:00.000Z"},
            {"approvalId":fixture_id(0xe011),"requestId":fixture_id(0xe021),"machineId":fixture_id(0xe000),
             "state":"enrolled","endReason":null,"allowedWorkspaces":[],"baseGeneration":7,
             "codeExpiresAt":"2027-01-15T08:10:00.000Z","machineClientId":fixture_id(0xe031),
             "lastGrantAt":"2027-01-15T08:00:00.000Z","endedAt":null,"createdAt":"2027-01-15T08:00:01.000Z"},
            {"approvalId":fixture_id(0xe012),"requestId":fixture_id(0xe022),"machineId":fixture_id(0xe000),
             "state":"approved","endReason":null,"allowedWorkspaces":[],"baseGeneration":7,
             "codeExpiresAt":"2027-01-15T08:10:00.000Z","machineClientId":null,
             "lastGrantAt":null,"endedAt":null,"createdAt":"2027-01-15T08:00:02.000Z"}
        ])
    );
    assert!(value.get("machineCredentials").is_none());
    assert_eq!(writes(&mut connection).await, before);
}

#[tokio::test]
async fn corrupt_machine_columns_refuse_instead_of_becoming_complete_observations() {
    for sql in [
        "UPDATE login_machines SET enrollment_generation=-1",
        "UPDATE login_machines SET state='paused'",
        "UPDATE login_machines SET label=' Observed machine '",
        "UPDATE login_machine_approvals SET allowed_workspaces='invalid-json'",
        "UPDATE login_machine_approvals SET code_expires_at='not-a-timestamp'",
        "UPDATE login_machine_approvals SET permission_checked='machine:other'",
        "UPDATE login_machine_approvals SET last_grant_at=NULL WHERE state='enrolled'",
        "UPDATE login_machine_approvals SET ended_at=NULL WHERE state='ended'",
    ] {
        // Arrange: a single corrupt persisted field in an otherwise valid store.
        let mut connection = machine_store().await;
        connection.execute(sql).await.unwrap();
        let before = writes(&mut connection).await;
        // Act / Assert: observation refuses, without repairing or rewriting data.
        assert!(
            decode_snapshot(&stored_snapshot(&mut connection).await, &organization()).is_err(),
            "corrupt machine field was accepted"
        );
        assert_eq!(writes(&mut connection).await, before);
    }
}

#[tokio::test]
async fn automatic_idle_records_observe_with_exact_origin_validation() {
    // Arrange: the same legitimate origin shape emitted by idle expiry.
    let mut connection = stamped_store(0).await;
    query::<Sqlite>("INSERT INTO signin_change_records VALUES(?,?,'machineExpired','automatic',NULL,NULL,?,?,?)")
        .bind(fixture_id(0xf001)).bind("idle-expiry-record").bind(fixture_id(0xe013)).bind(NOW).bind(NOW)
        .execute(&mut connection).await.unwrap();
    let before = writes(&mut connection).await;
    // Act / Assert: automatic records are readable, but never gain an actor.
    let observed =
        decode_snapshot(&stored_snapshot(&mut connection).await, &organization()).unwrap();
    assert_eq!(observed.records.len(), 1);
    assert_eq!(
        serde_json::to_value(observed.records[0].kind).unwrap(),
        json!("machineExpired")
    );
    assert_eq!(writes(&mut connection).await, before);
    query::<Sqlite>("UPDATE signin_change_records SET actor_person_id=?")
        .bind(PERSON)
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(decode_snapshot(&stored_snapshot(&mut connection).await, &organization()).is_err());
    connection.execute("UPDATE signin_change_records SET actor_person_id=NULL,permission_checked='machine:enroll'").await.unwrap();
    let invalid_permission = stored_snapshot(&mut connection).await;
    assert!(decode_snapshot(&invalid_permission, &organization()).is_err());
}

#[tokio::test]
async fn approval_order_uses_checked_instants_across_valid_timestamp_forms_and_id_ties() {
    // Arrange: raw RFC3339 text sorts the later fractional values before the
    // whole second. Equal instants also need a deterministic approval-ID tie.
    let mut connection = machine_store().await;
    for (id, created) in [
        (0xe011, "2027-01-15T08:00:00Z"),
        (0xe012, "2027-01-15T08:00:00.001+00:00"),
        (0xe013, "2027-01-15T08:00:00.001Z"),
    ] {
        query::<Sqlite>("UPDATE login_machine_approvals SET created_at=?,updated_at='2027-01-15T08:00:02Z' WHERE approval_id=?")
            .bind(created).bind(fixture_id(id)).execute(&mut connection).await.unwrap();
    }
    connection
        .execute("UPDATE login_machine_approvals SET ended_at='2027-01-15T08:00:02Z' WHERE state='ended'")
        .await
        .unwrap();
    let before = writes(&mut connection).await;
    // Act: all source rows pass the actual owning typed decoder.
    let observed =
        decode_snapshot(&stored_snapshot(&mut connection).await, &organization()).unwrap();
    let encoded = serde_json::to_value(observed).unwrap();
    let approvals = encoded["machineApprovals"].as_array().unwrap();
    // Assert: chronological order and ID ties follow checked instants, not text.
    assert_eq!(
        approvals
            .iter()
            .map(|row| row["approvalId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [fixture_id(0xe011), fixture_id(0xe012), fixture_id(0xe013)]
    );
    assert_eq!(
        approvals
            .iter()
            .map(|row| row["createdAt"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "2027-01-15T08:00:00.000Z",
            "2027-01-15T08:00:00.001Z",
            "2027-01-15T08:00:00.001Z"
        ]
    );
    assert_eq!(writes(&mut connection).await, before);
}
