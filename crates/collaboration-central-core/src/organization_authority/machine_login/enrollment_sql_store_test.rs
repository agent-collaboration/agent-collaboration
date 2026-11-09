//! The decisions over the real Tokio store (sqlx-turso), initialized by the
//! staged v3/v4. Each test drives a public decision entry; the store is read
//! back through the port, and as raw rows where nothing may change.
use super::super::device_login_decision::decide_device_login;
use super::super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::super::enrollment_records::{EnrollmentGeneration, StoredEnrollmentState};
use super::super::machine_token_decision::decide_machine_token;
use super::super::sql_store_fixture::{FixturePort, SqlStoreFixture, TestError};
use super::super::test_fixtures::{
    CODE_LIFETIME_S, CountingIds, MACHINE, NOW_MS, NOW_S, TestClock, WORKSPACE_A, WORKSPACE_B,
    approval, client, enrollment, installation, settings, uuid7,
};
use crate::identity_contract::{
    AllowanceState, DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision, DeviceLoginRefusal,
    MachineTokenDecision, MachineTokenRefusal,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use std::sync::Arc;

const REQUEST: u64 = 0x30;

async fn facts(port: &FixturePort, query: EnrollmentQuery) -> EnrollmentFacts {
    port.transact(query, |facts| Decided::unchanged(facts.clone()))
        .await
        .unwrap()
}

async fn approve(
    port: &FixturePort,
    request: DecideDeviceLogin,
    now_ms: i64,
    ids: u64,
) -> Result<DeviceLoginDecision, MachineEnrollmentFailure> {
    decide_device_login(
        port,
        request,
        now_ms,
        TestClock::at(now_ms),
        Arc::new(CountingIds::from(ids)),
    )
    .await
}

async fn grant(
    port: &FixturePort,
    id: &EnrollmentId,
    client_suffix: u64,
    now_ms: i64,
    ids: u64,
) -> MachineTokenDecision {
    decide_machine_token(
        port,
        DecideMachineToken {
            organization_id: OrganizationId::try_from(uuid7(0x01)).unwrap(),
            enrollment_id: id.clone(),
            client_id: client(client_suffix),
        },
        TestClock::at(now_ms),
        Arc::new(CountingIds::from(ids)),
        settings(),
    )
    .await
    .unwrap()
}

fn approved_id(answer: Result<DeviceLoginDecision, MachineEnrollmentFailure>) -> EnrollmentId {
    match answer {
        Ok(DeviceLoginDecision::Approved { enrollment_id }) => enrollment_id,
        other => panic!("expected Approved, got {other:?}"),
    }
}

fn of_installation() -> EnrollmentQuery {
    EnrollmentQuery {
        installation: Some(installation(MACHINE)),
        ..EnrollmentQuery::default()
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn the_staged_corpus_initializes_once_and_a_second_initialization_applies_nothing()
-> Result<(), TestError> {
    let fixture = SqlStoreFixture::pre_b3(Vec::new()).await?;
    assert_eq!(fixture.initialize_b3().await?, 2);
    assert_eq!(fixture.initialize_b3().await?, 0);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn an_approval_and_its_first_grant_round_trip_through_the_real_store() -> Result<(), TestError>
{
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A, WORKSPACE_B]).await?;
    let port = fixture.port();

    let enrollment_id = approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B]),
            NOW_MS,
            0xa000,
        )
        .await,
    );

    let approved = facts(
        &port,
        EnrollmentQuery {
            enrollment: Some(enrollment_id.clone()),
            ..of_installation()
        },
    )
    .await;
    let stored = approved.enrollment.clone().unwrap();
    assert_eq!(stored.state, StoredEnrollmentState::Approved);
    assert_eq!(stored.approved_at_ms, NOW_MS);
    assert_eq!(stored.code_expires_at, UnixSeconds(NOW_S + CODE_LIFETIME_S));
    assert_eq!(stored.base_generation, EnrollmentGeneration(0));
    let row = approved.installation.clone().unwrap();
    assert_eq!(
        (row.generation, row.active),
        (EnrollmentGeneration(0), None)
    );
    assert_eq!(
        approved
            .allowances
            .iter()
            .map(|allowance| allowance.state.clone())
            .collect::<Vec<_>>(),
        vec![AllowanceState::Active, AllowanceState::Active]
    );

    let answer = grant(
        &port,
        &enrollment_id,
        enrollment_suffix(&enrollment_id),
        NOW_MS + 1000,
        0xb000,
    )
    .await;

    assert!(matches!(
        answer,
        MachineTokenDecision::Grant {
            superseded: None,
            ..
        }
    ));
    let granted = facts(
        &port,
        EnrollmentQuery {
            enrollment: Some(enrollment_id.clone()),
            ..of_installation()
        },
    )
    .await;
    let row = granted.installation.unwrap();
    assert_eq!(
        (row.generation, row.active),
        (EnrollmentGeneration(1), Some(enrollment_id.clone()))
    );
    let enrolled = granted.enrollment.unwrap();
    assert!(matches!(
        enrolled.state,
        StoredEnrollmentState::Enrolled { .. }
    ));
    // The approval's own time never changes.
    assert_eq!(enrolled.approved_at_ms, NOW_MS);
    Ok(())
}

/// The enrollment's own machine client has the enrollment's ID.
fn enrollment_suffix(id: &EnrollmentId) -> u64 {
    u64::from_str_radix(id.as_str().get(24..).unwrap(), 16).unwrap()
}

/// The approved SQL keeps no request body, so a repeated request ID is refused
/// as pending the owner, and nothing is written. Not N03's success.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn a_repeated_request_is_pending_the_owner_on_the_real_store() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A]),
            NOW_MS,
            0xa000,
        )
        .await,
    );
    let before = fixture.rows().await?;

    let repeat = approve(
        &port,
        approval(REQUEST, MACHINE, &[WORKSPACE_A]),
        NOW_MS,
        0xa100,
    )
    .await;

    assert_eq!(repeat, Err(MachineEnrollmentFailure::RepeatPendingOwner));
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn refusals_write_nothing_to_the_real_store() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    let before = fixture.rows().await?;

    let unknown = approve(
        &port,
        approval(REQUEST, MACHINE, &[WORKSPACE_B]),
        NOW_MS,
        0xa000,
    )
    .await;
    let wrong_peer = approve(
        &port,
        DecideDeviceLogin {
            peer: fleet_sync_protocol::PeerId::try_from(uuid7(0x11)).unwrap(),
            ..approval(REQUEST + 1, MACHINE, &[WORKSPACE_A])
        },
        NOW_MS,
        0xa000,
    )
    .await;

    assert_eq!(
        (unknown, wrong_peer),
        (
            Ok(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::UnknownWorkspace {},
            }),
            Ok(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::NotPermitted {},
            })
        )
    );
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}

/// The records of one approval, a superseding first Grant and an automatic
/// expiry each keep their own key; a retried decision adds none.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn every_record_of_a_machine_keeps_its_own_key_on_the_real_store() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A, WORKSPACE_B]).await?;
    let port = fixture.port();
    let first = approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B]),
            NOW_MS,
            0xa000,
        )
        .await,
    );
    grant(
        &port,
        &first,
        enrollment_suffix(&first),
        NOW_MS + 1000,
        0xb000,
    )
    .await;
    let second = approved_id(
        approve(
            &port,
            approval(REQUEST + 1, MACHINE, &[WORKSPACE_A]),
            NOW_MS + 2000,
            0xa100,
        )
        .await,
    );
    grant(
        &port,
        &second,
        enrollment_suffix(&second),
        NOW_MS + 3000,
        0xb100,
    )
    .await;
    // A repeat Grant writes no record.
    grant(
        &port,
        &second,
        enrollment_suffix(&second),
        NOW_MS + 4000,
        0xb200,
    )
    .await;
    // Thirty days after the last Grant, the repeat at +4 s.
    let idle_ms = (NOW_S + 4 + 30 * 24 * 60 * 60) * 1000;
    let expired = grant(&port, &second, enrollment_suffix(&second), idle_ms, 0xb300).await;
    assert_eq!(
        expired,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::IdleExpired {},
        }
    );
    // A later observation of the expired enrollment writes nothing more.
    let after_expiry = fixture.rows().await?;
    grant(
        &port,
        &second,
        enrollment_suffix(&second),
        idle_ms + 1000,
        0xb400,
    )
    .await;
    assert_eq!(fixture.rows().await?, after_expiry);

    let records = change_records(&fixture).await?;
    // Record ids come from each decision's own ID block, so compare as a set.
    let mut kinds = records
        .iter()
        .map(|record| format!("{:?} {:?}", record.kind, record.origin))
        .collect::<Vec<_>>();
    kinds.sort();
    let approver = OrganizationChangeOrigin::Person {
        person_id: uuid7(0x02),
        permission_checked: "machine:enroll".to_owned(),
    };
    let automatic = OrganizationChangeOrigin::Automatic {};
    let mut expected = [
        (OrganizationChangeKind::AllowanceChanged, approver.clone()),
        (OrganizationChangeKind::AllowanceChanged, approver.clone()),
        (OrganizationChangeKind::MachineEnrolled, approver.clone()),
        (OrganizationChangeKind::MachineRevoked, approver.clone()),
        (OrganizationChangeKind::MachineEnrolled, approver),
        (OrganizationChangeKind::MachineExpired, automatic.clone()),
        (OrganizationChangeKind::AllowanceChanged, automatic.clone()),
        (OrganizationChangeKind::AllowanceChanged, automatic),
    ]
    .iter()
    .map(|(kind, origin)| format!("{kind:?} {origin:?}"))
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(kinds, expected);
    let mut keys = records
        .iter()
        .map(|record| record.request_id.as_str().to_owned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), records.len());
    Ok(())
}

async fn change_records(
    fixture: &SqlStoreFixture,
) -> Result<Vec<crate::organization_authority::OrganizationChangeRecord>, TestError> {
    use durable_steps::AtomicStore;
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                crate::organization_authority::change_records::read_page(scope, None, 100).await
            })
        })
        .await?)
}

/// BA1 on the real store: of two approvals at one generation, the first Grant
/// wins; the other is refused superseded and never activates.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn of_two_approvals_the_first_grant_wins_on_the_real_store() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    let a = approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A]),
            NOW_MS,
            0xa000,
        )
        .await,
    );
    let b = approved_id(
        approve(
            &port,
            approval(REQUEST + 1, MACHINE, &[WORKSPACE_A]),
            NOW_MS + 100,
            0xa100,
        )
        .await,
    );
    assert!(matches!(
        grant(&port, &b, enrollment_suffix(&b), NOW_MS + 1000, 0xb000).await,
        MachineTokenDecision::Grant { .. }
    ));
    let before = fixture.rows().await?;

    let late = grant(&port, &a, enrollment_suffix(&a), NOW_MS + 2000, 0xb100).await;

    assert_eq!(
        late,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::Superseded {},
        }
    );
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}

/// N10 on the real store: approved at minute 9, the code expires at minute 10.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn the_codes_own_deadline_lapses_an_approval_on_the_real_store() -> Result<(), TestError> {
    for (at_ms, lapses) in [(NOW_MS - 1, false), (NOW_MS, true), (NOW_MS + 1, true)] {
        let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
        let port = fixture.port();
        let request = DecideDeviceLogin {
            code_expires_at: UnixSeconds(NOW_S),
            ..approval(REQUEST, MACHINE, &[WORKSPACE_A])
        };
        let id = approved_id(approve(&port, request, NOW_MS - 60_000, 0xa000).await);
        let before = fixture.rows().await?;

        let answer = grant(&port, &id, enrollment_suffix(&id), at_ms, 0xb000).await;

        let stored = facts(
            &port,
            EnrollmentQuery {
                enrollment: Some(id.clone()),
                ..of_installation()
            },
        )
        .await;
        if lapses {
            assert_eq!(
                answer,
                MachineTokenDecision::Refuse {
                    reason: MachineTokenRefusal::EnrollmentNotActive {},
                },
                "at {at_ms}"
            );
            let lapsed = stored.enrollment.unwrap();
            assert_eq!(
                (lapsed.state, lapsed.ended_at_ms),
                (StoredEnrollmentState::Lapsed, Some(at_ms))
            );
            assert_eq!(
                stored.installation.unwrap().generation,
                EnrollmentGeneration(0)
            );
            // Only the enrollment row changed.
            let after = fixture.rows().await?;
            let changed = after
                .iter()
                .filter(|row| !before.contains(row))
                .collect::<Vec<_>>();
            assert_eq!(changed.len(), 1, "at {at_ms}: {changed:?}");
            assert!(changed[0].starts_with("machine_enrollments:"));
        } else {
            assert!(
                matches!(answer, MachineTokenDecision::Grant { .. }),
                "at {at_ms}"
            );
        }
    }
    Ok(())
}

/// A new owning handle over the same persistence reads the same facts.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn the_real_store_reads_back_the_same_after_reopening() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A, WORKSPACE_B]).await?;
    let port = fixture.port();
    let id = approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B]),
            NOW_MS,
            0xa000,
        )
        .await,
    );
    grant(&port, &id, enrollment_suffix(&id), NOW_MS + 1000, 0xb000).await;
    let query = EnrollmentQuery {
        enrollment: Some(id.clone()),
        ..of_installation()
    };
    let before = facts(&port, query.clone()).await;
    let rows = fixture.rows().await?;
    drop(port);

    let reopened = fixture.reopen().await?;

    let after = facts(&reopened.port(), query).await;
    assert_eq!(format!("{after:?}"), format!("{before:?}"));
    assert_eq!(reopened.rows().await?, rows);
    Ok(())
}

/// An engine refusal after real writes rolls the whole decision back: the
/// enrollment, allowance and record written first are gone too.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "a test asserts its outcomes and propagates fixture failures with ?"
)]
async fn a_write_failing_after_others_rolls_the_whole_transaction_back() -> Result<(), TestError> {
    use super::super::enrollment_records::{AllowanceRow, EnrollmentChange};
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    let id = approved_id(
        approve(
            &port,
            approval(REQUEST, MACHINE, &[WORKSPACE_A]),
            NOW_MS,
            0xa000,
        )
        .await,
    );
    let before = fixture.rows().await?;
    let stored = facts(
        &port,
        EnrollmentQuery {
            enrollment: Some(id.clone()),
            ..EnrollmentQuery::default()
        },
    )
    .await
    .enrollment
    .unwrap();

    let failed = port
        .transact(EnrollmentQuery::default(), move |_| {
            let mut duplicate = stored;
            duplicate.id = enrollment(0x7777);
            Decided::writing(
                (),
                vec![
                    EnrollmentChange::PutAllowance(AllowanceRow {
                        id: durable_steps::Uuid::parse_str(&uuid7(0x7001)).unwrap(),
                        installation: installation(MACHINE),
                        workspace: super::super::test_fixtures::workspace(WORKSPACE_B),
                        state: AllowanceState::Active,
                    }),
                    // The same request ID again: the engine's UNIQUE refuses it.
                    EnrollmentChange::InsertEnrollment(duplicate),
                ],
                NOW_MS + 1,
            )
        })
        .await;

    assert_eq!(failed, Err(MachineEnrollmentFailure::Unavailable));
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}
