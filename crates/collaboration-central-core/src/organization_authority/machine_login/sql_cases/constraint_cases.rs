//! The staged tables' own uniqueness, shown by the engine itself: raw inserts
//! that bypass every Rust check are refused, the store reads back unchanged,
//! and a valid control insert succeeds. Then the Rust checks on read: an
//! invalid stored row is refused, never coerced, and nothing is written.
use super::super::enrollment_port::{
    Decided, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::super::enrollment_records::EnrollmentChange;
use super::case_values::{
    FORMER, MACHINE, NOW_MS, WORKSPACE_A, b3_store, installation, typed, uuid7,
};
use super::expected_rows::{before, expect_unchanged};
use super::sql_case_host::{CaseHost, snapshot};
use super::{CaseFailure, CaseResult, check, check_eq};
use crate::identity_contract::FormerServiceId;

fn installation_row(id: &str, former: Option<&str>) -> String {
    let former = former.map_or_else(|| "NULL".to_owned(), |value| format!("'{value}'"));
    format!(
        "INSERT INTO machine_installations(id, former_service_id, label, enrollment_generation, \
         last_seen_at, created_at, updated_at) VALUES('{id}', {former}, 'm', 0, NULL, 1, 1)"
    )
}

/// One raw enrollment row; `None` columns are NULL.
#[derive(Clone)]
struct RawEnrollment {
    id: u64,
    installation: u64,
    request: String,
    state: &'static str,
    client: Option<u64>,
    last_grant: Option<i64>,
    ended: Option<i64>,
    end_reason: Option<&'static str>,
}
impl RawEnrollment {
    fn insert(&self) -> String {
        let text =
            |value: Option<String>| value.map_or_else(|| "NULL".to_owned(), |v| format!("'{v}'"));
        let number =
            |value: Option<i64>| value.map_or_else(|| "NULL".to_owned(), |v| v.to_string());
        format!(
            "INSERT INTO machine_enrollments(id, installation_id, request_id, state, end_reason, \
             approved_by_person_id, permission_checked, base_generation, code_expires_at, \
             machine_client_id, last_grant_at, ended_at, created_at, updated_at) VALUES('{}', \
             '{}', '{}', '{}', {}, '{}', 'machine:enroll', 0, 1800000600000, {}, {}, {}, \
             1800000000000, 1800000000000)",
            uuid7(self.id),
            uuid7(self.installation),
            self.request,
            self.state,
            text(self.end_reason.map(str::to_owned)),
            uuid7(0x02),
            text(self.client.map(uuid7)),
            number(self.last_grant),
            number(self.ended),
        )
    }
}

fn row(id: u64, request: u64, state: &'static str) -> RawEnrollment {
    RawEnrollment {
        id,
        installation: MACHINE,
        request: uuid7(request),
        state,
        client: None,
        last_grant: None,
        ended: None,
        end_reason: None,
    }
}

/// The probe must be refused by the engine as a uniqueness violation, leave
/// the store as it was, and a valid control insert must still go through.
async fn engine_refuses<Host: CaseHost>(host: &Host, probe: String, control: String) -> CaseResult {
    let rows = before(host).await?;
    let refused = host.execute(probe).await;
    let Err(error) = refused else {
        return Err(CaseFailure(
            "the engine accepted a row its uniqueness forbids".to_owned(),
        ));
    };
    check!(
        error.to_string().to_lowercase().contains("unique"),
        "not a uniqueness refusal: {error}"
    );
    expect_unchanged(host, &rows, "the refused insert").await?;
    host.execute(control).await?;
    check!(
        snapshot(host).await? != rows,
        "the control insert changed nothing"
    );
    Ok(())
}

pub(super) async fn second_enrolled<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[]).await?;
    host.execute(installation_row(&uuid7(MACHINE), None))
        .await?;
    let enrolled = |id: u64| RawEnrollment {
        client: Some(id),
        last_grant: Some(1_800_000_000_000),
        ..row(id, id + 0x100, "enrolled")
    };
    host.execute(enrolled(0x40).insert()).await?;
    // An approved one for the same installation is the control.
    engine_refuses(
        host,
        enrolled(0x41).insert(),
        row(0x42, 0x142, "approved").insert(),
    )
    .await
}

pub(super) async fn duplicate_former_id<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[]).await?;
    host.execute(installation_row(&uuid7(MACHINE), Some(FORMER)))
        .await?;
    engine_refuses(
        host,
        installation_row(&uuid7(0x11), Some(FORMER)),
        installation_row(&uuid7(0x12), Some("0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7e")),
    )
    .await
}

pub(super) async fn duplicate_allowance<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[]).await?;
    let allowance = |id: u64, workspace: u64| {
        format!(
            "INSERT INTO machine_allowances(id, installation_id, workspace_id, state, created_at, \
             updated_at) VALUES('{}', '{}', '{}', 'active', 1, 1)",
            uuid7(id),
            uuid7(MACHINE),
            uuid7(workspace)
        )
    };
    host.execute(allowance(0x300, 0x20)).await?;
    engine_refuses(host, allowance(0x301, 0x20), allowance(0x302, 0x21)).await
}

pub(super) async fn duplicate_request_or_client<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[]).await?;
    host.execute(installation_row(&uuid7(MACHINE), None))
        .await?;
    let revoked = RawEnrollment {
        client: Some(0x40),
        ended: Some(1),
        end_reason: Some("owner"),
        ..row(0x40, 0x140, "revoked")
    };
    host.execute(revoked.insert()).await?;
    // The same request ID.
    engine_refuses(
        host,
        row(0x41, 0x140, "approved").insert(),
        row(0x42, 0x142, "approved").insert(),
    )
    .await?;
    // The same machine client.
    let expired = |id: u64, client: u64| RawEnrollment {
        client: Some(client),
        ended: Some(1),
        ..row(id, id + 0x100, "expired")
    };
    engine_refuses(
        host,
        expired(0x43, 0x40).insert(),
        expired(0x44, 0x44).insert(),
    )
    .await
}

/// Each invalid stored enrollment is refused when read, with nothing written.
/// Each probe has its own installation, so a refusal is that row's own.
pub(super) async fn invalid_rows_refused<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[]).await?;
    let probes = [
        ("an unknown state", row(0x40, 0x140, "paused")),
        (
            "enrolled without a client",
            RawEnrollment {
                last_grant: Some(1_000),
                ..row(0x41, 0x141, "enrolled")
            },
        ),
        (
            "approved with an end",
            RawEnrollment {
                ended: Some(1),
                ..row(0x42, 0x142, "approved")
            },
        ),
        (
            "revoked for an unknown reason",
            RawEnrollment {
                client: Some(0x43),
                ended: Some(1),
                end_reason: Some("bored"),
                ..row(0x43, 0x143, "revoked")
            },
        ),
        (
            "a last Grant that isn't whole seconds",
            RawEnrollment {
                client: Some(0x44),
                last_grant: Some(1_001),
                ..row(0x44, 0x144, "enrolled")
            },
        ),
        (
            "a request id that isn't a UUIDv7",
            RawEnrollment {
                request: "not-a-uuid".to_owned(),
                ..row(0x45, 0x145, "approved")
            },
        ),
    ];
    for (offset, (probe, raw)) in (0_u64..).zip(probes) {
        let machine = 0x60 + offset;
        host.execute(installation_row(&uuid7(machine), None))
            .await?;
        host.execute(
            RawEnrollment {
                installation: machine,
                ..raw.clone()
            }
            .insert(),
        )
        .await?;
        let rows = before(host).await?;
        let enrollment = typed(uuid7(raw.id))?;

        let read = host
            .port()
            .transact(
                EnrollmentQuery {
                    enrollment: Some(enrollment),
                    ..EnrollmentQuery::default()
                },
                |facts| Decided::unchanged(facts.enrollment.is_some()),
            )
            .await;

        check_eq!(read, Err(MachineEnrollmentFailure::CorruptStore), "{probe}");
        expect_unchanged(host, &rows, probe).await?;
    }
    // A negative generation on an installation.
    host.execute(installation_row(&uuid7(0x70), None).replace(", 'm', 0,", ", 'm', -1,"))
        .await?;
    let read = host
        .port()
        .transact(
            EnrollmentQuery {
                installation: Some(installation(0x70)?),
                ..EnrollmentQuery::default()
            },
            |facts| Decided::unchanged(facts.installation.is_some()),
        )
        .await;
    check_eq!(
        read,
        Err(MachineEnrollmentFailure::CorruptStore),
        "a negative generation"
    );
    Ok(())
}

/// A former service id is set once: linking another to a machine that has one
/// fails the transaction, and nothing changes.
pub(super) async fn former_id_never_replaced<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    host.execute(installation_row(&uuid7(MACHINE), Some(FORMER)))
        .await?;
    let rows = before(host).await?;
    let other = typed::<FormerServiceId>("0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7e".to_owned())?;
    let machine = installation(MACHINE)?;

    let replaced = host
        .port()
        .transact(EnrollmentQuery::default(), move |_| {
            Decided::writing(
                (),
                vec![EnrollmentChange::LinkFormerService {
                    former: other,
                    installation: machine,
                }],
                NOW_MS,
            )
        })
        .await;

    check_eq!(
        replaced,
        Err(MachineEnrollmentFailure::CorruptStore),
        "the replacement"
    );
    expect_unchanged(host, &rows, "the replacement").await
}
