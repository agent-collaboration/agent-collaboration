//! B3's switch on a Fleet built by today's 0001/0002 code, then the staged
//! v3/v4 through the migration framework. A refused switch leaves today's
//! schema, rows and history exactly as they were.
use super::case_values::{NOW_MS, uuid7};
use super::expected_rows::{Installation, expect_rows};
use super::sql_case_host::{CaseHost, LegacyMachine};
use super::{CaseFailure, CaseResult, check, check_eq};
use crate::migration_failure::MigrationFailure;

const V4_SERVICE: &str = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";

fn machine(peer: u64, service: &str, last_seen: Option<&str>, created_at: &str) -> LegacyMachine {
    LegacyMachine {
        peer: uuid7(peer),
        service: service.to_owned(),
        label: format!("machine {peer}"),
        last_seen: last_seen.map(str::to_owned),
        created_at: created_at.to_owned(),
    }
}

async fn history<Host: CaseHost>(host: &Host) -> CaseResult<Vec<String>> {
    Ok(host
        .select(
            "SELECT json_array(version, hex(checksum)) AS value FROM central_schema_migrations \
             WHERE corpus = 'central-registry' ORDER BY version"
                .to_owned(),
        )
        .await?)
}

async fn legacy_rows<Host: CaseHost>(host: &Host) -> CaseResult<Vec<String>> {
    Ok(host
        .select(
            "SELECT json_array(machine_id, service_id, machine_label, access_client_id, \
             credential_hash, state, last_seen_at, created_at) AS value FROM machine_enrollments \
             ORDER BY machine_id"
                .to_owned(),
        )
        .await?)
}

pub(super) async fn todays_machines_convert_once<Host: CaseHost>(host: &Host) -> CaseResult {
    host.initialize_pre_b3(vec![
        machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00.123Z"),
        machine(
            0x11,
            &uuid7(0x911),
            Some("2026-11-08T13:30:00.5+01:00"),
            "2026-11-08T12:00:00Z",
        ),
    ])
    .await?;
    let old_history = history(host).await?;

    check_eq!(host.initialize_b3().await?, 2, "the switch");

    // Ids, labels, former ids and exact-millisecond times are kept; the new
    // enrollment and allowance tables start empty; nothing else is written.
    let converted = |peer: u64, former: String, last_seen_at: Option<i64>, created_at: i64| {
        Installation {
            machine: peer,
            former: Some(former),
            label: format!("machine {peer}"),
            generation: 0,
            last_seen_at,
            created_at,
            updated_at: NOW_MS,
        }
        .entry()
    };
    let rows = vec![
        converted(0x10, V4_SERVICE.to_owned(), None, 1_794_139_200_123),
        converted(
            0x11,
            uuid7(0x911),
            Some(1_794_141_000_500),
            1_794_139_200_000,
        ),
    ];
    expect_rows(host, rows.clone(), "the converted store").await?;
    check!(
        host.select("SELECT access_client_id AS value FROM machine_enrollments".to_owned())
            .await
            .is_err(),
        "the Access columns survived the switch"
    );
    // Versions 1 and 2 keep their checksums; 3 and 4 are recorded.
    let new_history = history(host).await?;
    check_eq!(new_history.get(..2), old_history.get(..), "today's history");
    check_eq!(new_history.len(), 4, "the history's length");

    // A second initialization applies nothing and changes nothing.
    check_eq!(host.initialize_b3().await?, 0, "a second switch");
    expect_rows(host, rows, "after a second switch").await?;
    check_eq!(
        history(host).await?,
        new_history,
        "the history after a second switch"
    );
    Ok(())
}

pub(super) async fn empty_fleet_converts<Host: CaseHost>(host: &Host) -> CaseResult {
    host.initialize_pre_b3(Vec::new()).await?;
    check_eq!(host.initialize_b3().await?, 2, "the switch");
    expect_rows(host, Vec::new(), "the converted empty store").await?;
    check_eq!(host.initialize_b3().await?, 0, "a second switch");
    Ok(())
}

/// The switch refuses at `version` for `cause`, and today's schema, rows and
/// history are unchanged. Returns the rollback's reason.
async fn conversion_refused<Host: CaseHost>(
    host: &Host,
    version: u64,
    cause: &str,
) -> CaseResult<String> {
    let rows = legacy_rows(host).await?;
    let old_history = history(host).await?;

    let Err(refused) = host.initialize_b3().await else {
        return Err(CaseFailure("the switch applied".to_owned()));
    };
    let failure = MigrationFailure::from_rollback(&refused).ok_or_else(|| {
        CaseFailure(format!(
            "not a migration rollback naming its version: {refused}"
        ))
    })?;
    check_eq!(
        (failure.corpus.as_str(), failure.version),
        ("central-registry", version),
        "the refused migration"
    );
    check!(
        failure.reason.contains(cause),
        "the refusal names no {cause}: {}",
        failure.reason
    );

    check_eq!(legacy_rows(host).await?, rows, "today's machine rows");
    check_eq!(history(host).await?, old_history, "today's history");
    check!(
        host.select("SELECT id AS value FROM machine_installations".to_owned())
            .await
            .is_err(),
        "the new installations table survived the rollback"
    );
    Ok(failure.reason)
}

/// The refusal names every row that can't convert, by its id and offending
/// columns, so the owner's switch can be fixed from the error alone. It shows
/// no stored value, and the whole switch rolls back.
pub(super) async fn refusal_names_every_row<Host: CaseHost>(host: &Host) -> CaseResult {
    let unparseable = "yesterday";
    let sub_millisecond = "2026-11-08T12:00:00.1234Z";
    let bad_last_seen = "2026-11-08T13:30:00.5009+01:00";
    let invalid_id = "machine-id-that-is-not-a-uuid";
    host.initialize_pre_b3(vec![
        machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00Z"),
        machine(0x11, &uuid7(0x911), None, unparseable),
        machine(0x12, &uuid7(0x912), Some(bad_last_seen), sub_millisecond),
        LegacyMachine {
            peer: invalid_id.to_owned(),
            ..machine(0x13, &uuid7(0x913), None, "2026-11-08T12:00:00Z")
        },
    ])
    .await?;

    let reason = conversion_refused(host, 3, "refuses 3 legacy machine row(s)").await?;

    for named in [
        format!(
            "machine {}: machine_enrollments.created_at (is not an RFC 3339 time)",
            uuid7(0x11)
        ),
        format!(
            "machine {}: machine_enrollments.last_seen_at (is finer than a millisecond), \
             machine_enrollments.created_at (is finer than a millisecond)",
            uuid7(0x12)
        ),
        "row 4 in machine_id order: machine_enrollments.machine_id (is not a canonical UUIDv7)"
            .to_owned(),
    ] {
        check!(reason.contains(&named), "{named} missing from: {reason}");
    }
    // The valid row isn't named, and no stored value appears.
    check!(
        !reason.contains(&uuid7(0x10)),
        "the valid row is named: {reason}"
    );
    for value in [
        unparseable,
        sub_millisecond,
        bad_last_seen,
        invalid_id,
        "legacy-hash",
        "legacy-client",
        V4_SERVICE,
        "machine 17",
    ] {
        check!(!reason.contains(value), "{value} shown in: {reason}");
    }
    Ok(())
}

pub(super) async fn unfinished_legacy_operation_refuses<Host: CaseHost>(
    host: &Host,
    revoke: bool,
) -> CaseResult {
    host.initialize_pre_b3(vec![machine(
        0x10,
        V4_SERVICE,
        None,
        "2026-11-08T12:00:00Z",
    )])
    .await?;
    host.admit_legacy_task(revoke).await?;
    conversion_refused(host, 3, "is unfinished").await.map(drop)
}

/// A failure after v3's DROP and copy (here v4, whose table already exists)
/// rolls the whole switch back: today's table and rows return untouched.
pub(super) async fn failure_after_destructive_step<Host: CaseHost>(host: &Host) -> CaseResult {
    host.initialize_pre_b3(vec![
        machine(0x10, V4_SERVICE, None, "2026-11-08T12:00:00Z"),
        machine(0x11, &uuid7(0x911), None, "2026-11-08T12:00:01Z"),
    ])
    .await?;
    host.execute("CREATE TABLE machine_allowances (id TEXT)".to_owned())
        .await?;
    conversion_refused(host, 4, "machine_allowances")
        .await
        .map(drop)
}
