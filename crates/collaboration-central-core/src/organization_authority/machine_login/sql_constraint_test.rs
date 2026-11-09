//! The staged tables' own uniqueness, shown by the engine itself: raw inserts
//! that bypass every Rust check are refused, the store reads back unchanged,
//! and a valid control insert succeeds. Then the Rust checks on read: an
//! invalid stored row is refused, never coerced, and nothing is written.
use super::super::enrollment_port::{
    Decided, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::super::enrollment_records::EnrollmentChange;
use super::super::sql_store_fixture::{SqlStoreFixture, TestError};
use super::super::test_fixtures::{MACHINE, NOW_MS, WORKSPACE_A, enrollment, installation, uuid7};
use crate::identity_contract::FormerServiceId;

const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000010";

fn installation_row(id: &str, former: Option<&str>) -> String {
    let former = former.map_or_else(|| "NULL".to_owned(), |value| format!("'{value}'"));
    format!(
        "INSERT INTO machine_installations(id, former_service_id, label, enrollment_generation, \
         last_seen_at, created_at, updated_at) VALUES('{id}', {former}, 'm', 0, NULL, 1, 1)"
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "one raw row names every column the probe sets"
)]
fn enrollment_row(
    id: &str,
    installation: &str,
    request: &str,
    state: &str,
    client: Option<&str>,
    last_grant: Option<i64>,
    ended: Option<i64>,
    end_reason: Option<&str>,
) -> String {
    let text = |value: Option<&str>| value.map_or_else(|| "NULL".to_owned(), |v| format!("'{v}'"));
    let number = |value: Option<i64>| value.map_or_else(|| "NULL".to_owned(), |v| v.to_string());
    format!(
        "INSERT INTO machine_enrollments(id, installation_id, request_id, state, end_reason, \
         approved_by_person_id, permission_checked, base_generation, code_expires_at, \
         machine_client_id, last_grant_at, ended_at, created_at, updated_at) VALUES('{id}', \
         '{installation}', '{request}', '{state}', {}, '{}', 'machine:enroll', 0, 1800000600000, \
         {}, {}, {}, 1800000000000, 1800000000000)",
        text(end_reason),
        uuid7(0x02),
        text(client),
        number(last_grant),
        number(ended),
    )
}

/// The probe must be refused by the engine, leave the store as it was, and a
/// valid insert must still go through.
async fn assert_engine_refuses(
    fixture: &SqlStoreFixture,
    probe: String,
    control: String,
) -> Result<(), TestError> {
    let before = fixture.rows().await?;
    let refused = fixture.execute(probe).await;
    let error = refused.expect_err("the engine accepted a row its uniqueness forbids");
    assert!(
        error.to_string().to_lowercase().contains("unique"),
        "not a uniqueness refusal: {error}"
    );
    assert_eq!(fixture.rows().await?, before);
    fixture.execute(control).await?;
    assert_ne!(fixture.rows().await?, before);
    Ok(())
}

#[tokio::test]
async fn the_engine_refuses_a_second_enrolled_enrollment_for_one_installation()
-> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[]).await?;
    fixture
        .execute(installation_row(INSTALLATION, None))
        .await?;
    fixture
        .execute(enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "enrolled",
            Some(&uuid7(0x40)),
            Some(1_800_000_000_000),
            None,
            None,
        ))
        .await?;

    assert_engine_refuses(
        &fixture,
        enrollment_row(
            &uuid7(0x41),
            INSTALLATION,
            &uuid7(0x141),
            "enrolled",
            Some(&uuid7(0x41)),
            Some(1_800_000_000_000),
            None,
            None,
        ),
        // An approved one for the same installation is fine.
        enrollment_row(
            &uuid7(0x42),
            INSTALLATION,
            &uuid7(0x142),
            "approved",
            None,
            None,
            None,
            None,
        ),
    )
    .await
}

#[tokio::test]
async fn the_engine_refuses_a_duplicate_former_service_id() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[]).await?;
    let former = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";
    fixture
        .execute(installation_row(INSTALLATION, Some(former)))
        .await?;

    assert_engine_refuses(
        &fixture,
        installation_row(&uuid7(0x11), Some(former)),
        installation_row(&uuid7(0x12), Some("0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7e")),
    )
    .await
}

#[tokio::test]
async fn the_engine_refuses_a_second_allowance_for_one_installation_and_workspace()
-> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[]).await?;
    let allowance = |id: u64, workspace: u64| {
        format!(
            "INSERT INTO machine_allowances(id, installation_id, workspace_id, state, created_at, \
             updated_at) VALUES('{}', '{INSTALLATION}', '{}', 'active', 1, 1)",
            uuid7(id),
            uuid7(workspace)
        )
    };
    fixture.execute(allowance(0x300, 0x20)).await?;

    assert_engine_refuses(&fixture, allowance(0x301, 0x20), allowance(0x302, 0x21)).await
}

#[tokio::test]
async fn the_engine_refuses_a_duplicate_request_id_or_machine_client() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[]).await?;
    fixture
        .execute(installation_row(INSTALLATION, None))
        .await?;
    fixture
        .execute(enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "revoked",
            Some(&uuid7(0x40)),
            None,
            Some(1),
            Some("owner"),
        ))
        .await?;

    assert_engine_refuses(
        &fixture,
        enrollment_row(
            &uuid7(0x41),
            INSTALLATION,
            &uuid7(0x140),
            "approved",
            None,
            None,
            None,
            None,
        ),
        enrollment_row(
            &uuid7(0x42),
            INSTALLATION,
            &uuid7(0x142),
            "approved",
            None,
            None,
            None,
            None,
        ),
    )
    .await?;
    assert_engine_refuses(
        &fixture,
        enrollment_row(
            &uuid7(0x43),
            INSTALLATION,
            &uuid7(0x143),
            "expired",
            Some(&uuid7(0x40)),
            None,
            Some(1),
            None,
        ),
        enrollment_row(
            &uuid7(0x44),
            INSTALLATION,
            &uuid7(0x144),
            "expired",
            Some(&uuid7(0x44)),
            None,
            Some(1),
            None,
        ),
    )
    .await
}

/// Each invalid stored enrollment is refused when read, with nothing written.
#[tokio::test]
async fn an_invalid_stored_enrollment_is_refused_on_read_never_coerced() -> Result<(), TestError> {
    let rows = [
        // An unknown state.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "paused",
            None,
            None,
            None,
            None,
        ),
        // Enrolled without a client.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "enrolled",
            None,
            Some(1_000),
            None,
            None,
        ),
        // Approved with an end.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "approved",
            None,
            None,
            Some(1),
            None,
        ),
        // Revoked for an unknown reason.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "revoked",
            Some(&uuid7(0x40)),
            None,
            Some(1),
            Some("bored"),
        ),
        // A last Grant that isn't whole seconds.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            &uuid7(0x140),
            "enrolled",
            Some(&uuid7(0x40)),
            Some(1_001),
            None,
            None,
        ),
        // A request id that isn't a UUIDv7.
        enrollment_row(
            &uuid7(0x40),
            INSTALLATION,
            "not-a-uuid",
            "approved",
            None,
            None,
            None,
            None,
        ),
    ];
    for row in rows {
        let fixture = SqlStoreFixture::b3(&[]).await?;
        fixture
            .execute(installation_row(INSTALLATION, None))
            .await?;
        fixture.execute(row.clone()).await?;
        let before = fixture.rows().await?;

        let read = fixture
            .port()
            .transact(
                EnrollmentQuery {
                    enrollment: Some(enrollment(0x40)),
                    ..EnrollmentQuery::default()
                },
                |facts| Decided::unchanged(facts.enrollment.is_some()),
            )
            .await;

        assert_eq!(read, Err(MachineEnrollmentFailure::CorruptStore), "{row}");
        assert_eq!(fixture.rows().await?, before);
    }
    // A negative generation on the installation.
    let fixture = SqlStoreFixture::b3(&[]).await?;
    fixture
        .execute(installation_row(INSTALLATION, None).replace(", 'm', 0,", ", 'm', -1,"))
        .await?;
    let read = fixture
        .port()
        .transact(
            EnrollmentQuery {
                installation: Some(installation(MACHINE)),
                ..EnrollmentQuery::default()
            },
            |facts| Decided::unchanged(facts.installation.is_some()),
        )
        .await;
    assert_eq!(read, Err(MachineEnrollmentFailure::CorruptStore));
    Ok(())
}

/// A former service id is set once: linking another to a machine that has one
/// fails the transaction, and nothing changes.
#[tokio::test]
async fn a_former_service_id_is_never_replaced() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    fixture
        .execute(installation_row(
            INSTALLATION,
            Some("0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d"),
        ))
        .await?;
    let before = fixture.rows().await?;

    let replaced = fixture
        .port()
        .transact(EnrollmentQuery::default(), |_| {
            Decided::writing(
                (),
                vec![EnrollmentChange::LinkFormerService {
                    former: FormerServiceId::try_from(
                        "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7e".to_owned(),
                    )
                    .unwrap(),
                    installation: installation(MACHINE),
                }],
                NOW_MS,
            )
        })
        .await;

    assert_eq!(replaced, Err(MachineEnrollmentFailure::CorruptStore));
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}
